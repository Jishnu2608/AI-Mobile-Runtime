//! Running projects through framework adapters (Phase 3: ADP-001..003).
//!
//! * `project.run`: trusted folder → adapter by detection → toolchain from the plan (nothing may
//!   be missing) → the session's device (booted or switched) → adapter build / install / launch.
//!   Session: running (device) → building → launching → running (app), or `build_failed` /
//!   `launch_failed` with the problems.
//! * `project.reload`: hot / restart / full, only as the adapter declares.
//! * `project.stop`, `project.status` (events with a cursor, so agents fetch only what is new).
//! * `adapter.check`: the contract test kit live on the session's device.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{json, Value};
use umr_adapter::{Adapter, AdapterContext, AdapterError, DeviceTarget, Event, ReloadKind, Sink};
use umr_api::methods::{
    AdapterCheckParams, ProjectPlanParams, ReloadParams, RunParams, RunStatusParams,
    StartSessionParams,
};
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_session::Event as SessionEvent;

use crate::api_server::{ok, ApiState};
use crate::{device_api, session_api};

/// Every adapter the product ships.
#[must_use]
pub fn adapters() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(umr_adapter_gradle::GradleAndroid),
        Box::new(umr_adapter_flutter::Flutter),
        Box::new(umr_adapter_react_native::ReactNative),
    ]
}

/// Adapter events kept for `project.status` (newest last).
#[derive(Debug, Default)]
pub struct EventLog {
    next: u64,
    items: VecDeque<(u64, Value)>,
}

impl EventLog {
    const KEEP: usize = 500;

    pub(crate) fn push(&mut self, e: &Event) {
        self.next += 1;
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
        v["seq"] = json!(self.next);
        v["at_ms"] = json!(u64::try_from(ms).unwrap_or(0));
        self.items.push_back((self.next, v));
        while self.items.len() > Self::KEEP {
            self.items.pop_front();
        }
    }

    /// Events after `since` (at most `limit`, the newest ones) and the cursor to pass next time.
    #[must_use]
    pub fn since(&self, since: u64, limit: usize) -> (Vec<Value>, u64) {
        let v: Vec<Value> = self
            .items
            .iter()
            .filter(|(s, _)| *s > since)
            .map(|(_, v)| v.clone())
            .collect();
        let skip = v.len().saturating_sub(limit);
        (v.into_iter().skip(skip).collect(), self.next)
    }
}

/// What runs now.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunInfo {
    pub adapter: String,
    pub project: String,
    pub package: String,
    pub serial: String,
    pub group: String,
}

fn fail(code: i64, msg: impl Into<String>) -> RpcError {
    RpcError::new(code, msg)
}

fn events_sink(log: &Arc<Mutex<EventLog>>) -> Sink {
    let log = log.clone();
    Arc::new(move |e: Event| {
        if let Event::Problem(p) = &e {
            tracing::info!(file = ?p.file, line = ?p.line, "problem: {}", p.message);
        }
        log.lock().unwrap_or_else(PoisonError::into_inner).push(&e);
    })
}

/// JDK and Flutter folders from the project's plan; refuses when something is still missing.
/// JDK, Flutter and Node folders (the last may be a user install, used read-only).
struct Tools {
    jdk: Option<PathBuf>,
    flutter: Option<PathBuf>,
    node: Option<PathBuf>,
}

fn toolchain(state: &ApiState, path: &str) -> Result<Tools, RpcError> {
    let (_, plan) = crate::api_server::build_plan(
        state,
        &ProjectPlanParams {
            path: path.to_string(),
            rescan: false,
        },
    )?;
    let missing: Vec<String> = plan
        .items
        .iter()
        .filter(|i| matches!(i.action, umr_resolver::plan::Action::Install))
        // preview parts: the session boots an installed image; a newer one is not needed to build
        .filter(|i| {
            !matches!(
                i.component.as_str(),
                "android-system-image" | "android-emulator"
            )
        })
        .map(|i| i.title.clone())
        .collect();
    if !missing.is_empty() {
        return Err(fail(
            codes::UNAVAILABLE,
            format!(
                "still missing for this project: {}; run `umr install {path}` first",
                missing.join(", ")
            ),
        ));
    }
    let reuse = |c: &str| {
        plan.items
            .iter()
            .find(|i| i.component == c)
            .and_then(|i| match &i.action {
                umr_resolver::plan::Action::Reuse { path, .. } => Some(PathBuf::from(path)),
                umr_resolver::plan::Action::Install => None,
            })
    };
    // Node's plan path may be the folder or node.exe itself
    let node = reuse("node").map(|p| {
        if p.is_file() {
            p.parent().map(Path::to_path_buf).unwrap_or(p)
        } else {
            p
        }
    });
    Ok(Tools {
        jdk: reuse("jdk"),
        flutter: reuse("flutter"),
        node,
    })
}

/// Folders as written by the user may use `/` and relative parts; the Core needs one spelling.
fn normalise(path: &str) -> String {
    let p = Path::new(path);
    std::fs::canonicalize(p).map_or_else(
        |_| path.to_string(),
        |c| {
            let s = c.display().to_string();
            s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
        },
    )
}

fn context(
    state: &ApiState,
    project: &str,
    serial: &str,
    group: &str,
    tools: Tools,
) -> Result<AdapterContext, RpcError> {
    let sdk = state.toolchains.join("android-sdk");
    let adb = device_api::adb_server(state).map_err(|e| fail(codes::UNAVAILABLE, e))?;
    let ctx = umr_env::ScanContext::from_system(state.toolchains.clone());
    let sel = umr_env::recipe::Selection {
        jdk_home: tools.jdk.clone(),
        android_sdk: Some(sdk.clone()),
        flutter_root: tools.flutter.clone(),
        node_dir: tools.node.clone(),
        isolate_appdata: true,
        adb_port: Some(state.adb_port),
        ..Default::default()
    };
    umr_env::recipe::IsolationDirs::under(&state.toolchains)
        .create()
        .map_err(|e| fail(codes::INTERNAL_ERROR, e.to_string()))?;
    Ok(AdapterContext {
        project: PathBuf::from(project),
        env: umr_env::recipe::child_env(&ctx, &sel),
        sdk,
        jdk_home: tools.jdk,
        flutter_root: tools.flutter,
        device: DeviceTarget {
            serial: serial.to_string(),
            adb,
        },
        supervisor: state.supervisor.clone(),
        group: group.to_string(),
        log_dir: state.data_root.join("logs"),
    })
}

/// Stops the running app (if any) and its tools.
pub(crate) fn stop_run(state: &ApiState) -> Option<RunInfo> {
    // the watcher first, so no reload starts while the app stops
    drop(
        state
            .run_watch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take(),
    );
    let app = state
        .run_app
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    let info = state
        .run_info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(a) = app {
        a.stop();
    }
    if let Some(i) = &info {
        state.supervisor.stop_group(&i.group);
        tracing::info!(adapter = %i.adapter, package = %i.package, "run stopped");
    }
    info
}

fn problems_of(e: &AdapterError) -> Vec<Value> {
    match e {
        AdapterError::BuildFailed { problems, .. } => problems
            .iter()
            .filter_map(|p| serde_json::to_value(p).ok())
            .collect(),
        _ => Vec::new(),
    }
}

/// Picks the adapter, checks the toolchain, makes sure the device runs; returns the pieces.
fn prepare(
    state: &ApiState,
    raw: &str,
    path: &str,
    profile: Option<&str>,
    cold: bool,
) -> Result<(Box<dyn Adapter>, u8, AdapterContext, umr_session::Session), RpcError> {
    // trust is recorded as the user wrote the path; accept that spelling or the canonical one
    if crate::project_api::require_trusted(state, raw).is_err() {
        crate::project_api::require_trusted(state, path)?;
    }
    let mut all = adapters();
    let best = all
        .iter()
        .enumerate()
        .map(|(i, a)| (i, a.detect(Path::new(path))))
        .filter(|(_, c)| *c >= 40)
        .max_by_key(|(_, c)| *c);
    let Some((idx, confidence)) = best else {
        let d = umr_resolver::detect::detect(Path::new(path));
        return Err(fail(
            codes::INVALID_PARAMS,
            format!(
                "no adapter runs this folder yet (detected {:?}, confidence {}); supported: Android (Kotlin/Java, Gradle), Flutter, React Native / Expo",
                d.framework, d.confidence
            ),
        ));
    };
    let adapter = all.swap_remove(idx);
    let tools = toolchain(state, path)?;
    // the device: the session for this profile (booted, switched or recovered)
    let wanted = profile.unwrap_or("standard").to_string();
    session_api::ensure_device(
        state,
        &StartSessionParams {
            profile: wanted,
            api: None,
            cold,
            apk: None,
            package: None,
        },
    )?;
    let mut s = session_api::current(state)
        .ok_or_else(|| fail(codes::INTERNAL_ERROR, "no session after boot"))?;
    let serial = s
        .serial
        .clone()
        .ok_or_else(|| fail(codes::INTERNAL_ERROR, "the session has no device"))?;
    {
        let db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
        s.set_project(&db, path)
            .map_err(|e| fail(codes::INTERNAL_ERROR, e.to_string()))?;
    }
    let group = format!("run:{}", s.id);
    let ctx = context(state, path, &serial, &group, tools)?;
    let issues = adapter.validate(&ctx);
    if !issues.is_empty() {
        let text: Vec<String> = issues
            .iter()
            .map(|i| format!("{} (fix: {})", i.message, i.fix))
            .collect();
        return Err(fail(codes::UNAVAILABLE, text.join("; ")));
    }
    Ok((adapter, confidence, ctx, s))
}

pub fn run(state: &ApiState, id: Id, p: &RunParams) -> Response {
    let path = normalise(&p.path);
    stop_run(state);
    let (adapter, confidence, ctx, mut s) =
        match prepare(state, &p.path, &path, p.profile.as_deref(), p.cold) {
            Ok(v) => v,
            Err(e) => return Response::err(Some(id), e),
        };
    let log = state.run_events.clone();
    *log.lock().unwrap_or_else(PoisonError::into_inner) = EventLog::default();
    let sink = events_sink(&log);
    let _ = session_api::apply(state, &mut s, SessionEvent::Rebuild, Some(adapter.id()));
    let start = std::time::Instant::now();
    tracing::info!(adapter = adapter.id(), project = %path, "run");
    match adapter.start(&ctx, sink) {
        Ok(app) => {
            let package = app.package().to_string();
            s.app = Some(package.clone());
            let _ = session_api::apply(
                state,
                &mut s,
                SessionEvent::ArtifactReady,
                Some(adapter.id()),
            );
            let _ = session_api::apply(state, &mut s, SessionEvent::AppForeground, Some(&package));
            let info = RunInfo {
                adapter: adapter.id().into(),
                project: path.clone(),
                package: package.clone(),
                serial: ctx.device.serial.clone(),
                group: ctx.group.clone(),
            };
            *state.run_app.lock().unwrap_or_else(PoisonError::into_inner) = Some(app);
            let watching = if p.watch.unwrap_or(true) {
                match crate::watch::start(state, Path::new(&path), adapter.id()) {
                    Ok(w) => {
                        *state
                            .run_watch
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner) = Some(w);
                        true
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "file watching is off");
                        false
                    }
                }
            } else {
                false
            };
            *state
                .run_info
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(info.clone());
            ok(
                id,
                &json!({
                    "status": "running", "adapter": adapter.id(), "confidence": confidence,
                    "capabilities": adapter.capabilities(), "package": package, "watching": watching,
                    "serial": info.serial, "seconds": (start.elapsed().as_secs_f64() * 10.0).round() / 10.0,
                    "session": s,
                }),
            )
        }
        Err(e) => {
            let (ev, status) = match &e {
                AdapterError::BuildFailed { .. } => {
                    (Some(SessionEvent::BuildFailed), "build_failed")
                }
                AdapterError::InstallFailed(_) | AdapterError::LaunchFailed(_) => {
                    let _ = session_api::apply(state, &mut s, SessionEvent::ArtifactReady, None);
                    (Some(SessionEvent::LaunchFailed), "launch_failed")
                }
                _ => (Some(SessionEvent::BuildFailed), "failed"),
            };
            if let Some(ev) = ev {
                let _ = session_api::apply(state, &mut s, ev, Some(&e.to_string()));
            }
            state.supervisor.stop_group(&ctx.group);
            tracing::warn!(adapter = adapter.id(), error = %e, "run failed");
            ok(
                id,
                &json!({
                    "status": status, "adapter": adapter.id(), "error": e.to_string(),
                    "problems": problems_of(&e), "session": s,
                }),
            )
        }
    }
}

pub fn reload(state: &ApiState, id: Id, p: &ReloadParams) -> Response {
    let kind = match p.kind.as_deref().unwrap_or("hot") {
        "hot" => ReloadKind::Hot,
        "restart" => ReloadKind::Restart,
        "full" => ReloadKind::Full,
        other => {
            return Response::err(
                Some(id),
                fail(
                    codes::INVALID_PARAMS,
                    format!("unknown reload kind '{other}'; use hot, restart or full"),
                ),
            )
        }
    };
    match reload_kind(state, kind) {
        Ok(v) => ok(id, &v),
        Err(e) => Response::err(Some(id), e),
    }
}

/// Runs one reload (API and file watcher); the result describes what happened.
pub(crate) fn reload_kind(state: &ApiState, kind: ReloadKind) -> Result<Value, RpcError> {
    let sink = events_sink(&state.run_events);
    let mut guard = state.run_app.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(app) = guard.as_mut() else {
        return Err(fail(
            codes::UNAVAILABLE,
            "nothing is running; start it with `umr run <folder>`",
        ));
    };
    let mut s = session_api::current(state);
    let full = kind == ReloadKind::Full;
    if let Some(s) = s.as_mut() {
        let ev = if full {
            SessionEvent::Rebuild
        } else {
            SessionEvent::HotChange
        };
        let _ = session_api::apply(state, s, ev, Some(&format!("{kind:?}").to_lowercase()));
    }
    let r = app.reload(kind, &sink);
    drop(guard);
    match r {
        Ok(d) => {
            let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
            if let Some(s) = s.as_mut() {
                if full {
                    let _ = session_api::apply(state, s, SessionEvent::ArtifactReady, None);
                    let _ = session_api::apply(
                        state,
                        s,
                        SessionEvent::AppForeground,
                        Some(&format!("full reload {ms} ms")),
                    );
                } else {
                    let _ = session_api::apply(
                        state,
                        s,
                        SessionEvent::Reloaded,
                        Some(&format!("{ms} ms")),
                    );
                }
            }
            Ok(json!({ "status": "reloaded", "kind": kind, "ms": ms }))
        }
        Err(e) => {
            if let Some(s) = s.as_mut() {
                if full {
                    let _ = session_api::apply(
                        state,
                        s,
                        SessionEvent::BuildFailed,
                        Some(&e.to_string()),
                    );
                } else {
                    // the app keeps running the previous code
                    let _ = session_api::apply(
                        state,
                        s,
                        SessionEvent::Reloaded,
                        Some(&format!("failed: {e}")),
                    );
                }
            }
            let status = match e {
                AdapterError::Unsupported(_) => "unsupported",
                AdapterError::BuildFailed { .. } => "build_failed",
                _ => "failed",
            };
            Ok(
                json!({ "status": status, "kind": kind, "error": e.to_string(), "problems": problems_of(&e) }),
            )
        }
    }
}

pub fn stop(state: &ApiState, id: Id) -> Response {
    let info = stop_run(state);
    if let (Some(i), Some(mut s)) = (&info, session_api::current(state)) {
        if s.state == umr_session::State::Running && s.app.as_deref() == Some(i.package.as_str()) {
            let _ = session_api::apply(
                state,
                &mut s,
                SessionEvent::ProcessDied,
                Some("stopped with project.stop"),
            );
        }
    }
    ok(id, &json!({ "stopped": info }))
}

pub fn status(state: &ApiState, id: Id, p: &RunStatusParams) -> Response {
    let info = state
        .run_info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let alive = state
        .run_app
        .try_lock()
        .ok()
        .and_then(|mut g| g.as_mut().map(|a| a.is_alive()));
    let (events, cursor) = state
        .run_events
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .since(p.since.unwrap_or(0), p.limit.unwrap_or(100).min(500));
    ok(
        id,
        &json!({ "run": info, "alive": alive, "events": events, "cursor": cursor }),
    )
}

/// ADP-001: the contract kit live on the session's device.
pub fn check(state: &ApiState, id: Id, p: &AdapterCheckParams) -> Response {
    let path = normalise(&p.path);
    stop_run(state);
    let (adapter, _, ctx, _) = match prepare(state, &p.path, &path, p.profile.as_deref(), false) {
        Ok(v) => v,
        Err(e) => return Response::err(Some(id), e),
    };
    let port = ctx.device.adb.port;
    let serial = ctx.device.serial.clone();
    let probe = move |pkg: &str| umr_device::app::pid(port, &serial, pkg).is_some();
    tracing::info!(adapter = adapter.id(), project = %path, "adapter contract check");
    let report = umr_adapter::contract::run(adapter.as_ref(), &ctx, &probe);
    state.supervisor.stop_group(&ctx.group);
    ok(id, &json!({ "passed": report.passed(), "report": report }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_log_cursor_and_limit() {
        let mut log = EventLog::default();
        for i in 0..5 {
            log.push(&Event::Log {
                line: format!("l{i}"),
            });
        }
        let (all, cur) = log.since(0, 100);
        assert_eq!((all.len(), cur), (5, 5));
        assert_eq!(all[0]["event"], "log");
        assert_eq!(all[4]["seq"], 5);
        let (newer, cur2) = log.since(3, 100);
        assert_eq!(
            newer
                .iter()
                .map(|e| e["line"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["l3", "l4"]
        );
        assert_eq!(cur2, 5);
        let (last, _) = log.since(0, 2);
        assert_eq!(last[0]["line"], "l3", "limit keeps the newest");
        for i in 0..600 {
            log.push(&Event::Log {
                line: format!("x{i}"),
            });
        }
        assert_eq!(log.since(0, 1000).0.len(), EventLog::KEEP);
    }

    #[test]
    fn shipped_adapters_pass_the_static_contract() {
        let all = adapters();
        assert_eq!(
            all.iter().map(|a| a.id()).collect::<Vec<_>>(),
            ["gradle-android", "flutter", "react-native"]
        );
        for a in &all {
            assert_eq!(
                umr_adapter::contract::check_static(a.as_ref()),
                Vec::<String>::new(),
                "{}",
                a.id()
            );
        }
    }
}
