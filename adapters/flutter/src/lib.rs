//! Flutter adapter (ADP-003, Phase 3 step 3.3).
//!
//! Drives `flutter run --machine -d <serial>` (the Flutter tool's JSON protocol, POC-006) through
//! the Supervisor with stdin / stdout piped:
//!
//! * stdout: one JSON array per line, e.g. `[{"event":"app.started","params":{"appId":"…"}}]`;
//!   `app.progress` → progress, `app.log` / `daemon.logMessage` → log + problems,
//!   `app.stop` → the app ended. Plain lines (Gradle output) are parsed for problems.
//! * stdin: `[{"id":1,"method":"app.restart","params":{"appId":"…","fullRestart":false}}]`
//!   → hot reload (`fullRestart:false`) or hot restart (`true`); the reply carries
//!   `{"code":0,"message":"Reloaded 1 of 640 libraries in 312ms."}` (non-zero = compile error).
//! * Full reload = stop the tool and run again (native changes, new plugins).
//!
//! Environment: the isolated recipe with `FLUTTER_ROOT`, `PUB_CACHE` and `APPDATA` in the managed
//! folder (Flutter keeps `.flutter_settings` in `APPDATA`, POC-006 finding 10).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use umr_adapter::problems::parse_line;
use umr_adapter::{
    Adapter, AdapterContext, AdapterError, Capabilities, Event, Framework, Issue, Phase, Problem,
    ReloadKind, RunningApp, Severity, Sink,
};
use umr_supervisor::ProcessSpec;

/// Longest first run (Gradle build of the Android shell, pub get).
pub const START_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const RELOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Default, Clone, Copy)]
pub struct Flutter;

/// One decoded line of `flutter run --machine` output.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Event {
        name: String,
        params: Value,
    },
    Reply {
        id: u64,
        result: Option<Value>,
        error: Option<Value>,
    },
    Text(String),
}

/// Decodes a stdout line (`[{...}]` JSON, anything else is text).
#[must_use]
pub fn decode(line: &str) -> Vec<Message> {
    let t = line.trim();
    if t.starts_with('[') && t.ends_with(']') {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(t) {
            return items
                .into_iter()
                .map(|v| {
                    if let Some(name) = v.get("event").and_then(Value::as_str) {
                        Message::Event {
                            name: name.to_string(),
                            params: v.get("params").cloned().unwrap_or(Value::Null),
                        }
                    } else if let Some(id) = v.get("id").and_then(Value::as_u64) {
                        Message::Reply {
                            id,
                            result: v.get("result").cloned(),
                            error: v.get("error").cloned(),
                        }
                    } else {
                        Message::Text(v.to_string())
                    }
                })
                .collect();
        }
    }
    vec![Message::Text(line.to_string())]
}

/// `app.restart` request line.
#[must_use]
pub fn restart_request(id: u64, app_id: &str, full: bool) -> String {
    json!([{ "id": id, "method": "app.restart",
             "params": { "appId": app_id, "fullRestart": full, "pause": false, "reason": "manual" } }])
    .to_string()
}

#[must_use]
pub fn stop_request(id: u64, app_id: &str) -> String {
    json!([{ "id": id, "method": "app.stop", "params": { "appId": app_id } }]).to_string()
}

/// `applicationId` from `android/app/build.gradle(.kts)`.
#[must_use]
pub fn application_id(root: &Path) -> Option<String> {
    for f in ["build.gradle.kts", "build.gradle"] {
        let Ok(t) = std::fs::read_to_string(root.join("android").join("app").join(f)) else {
            continue;
        };
        for line in t.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("applicationId") {
                let v = rest
                    .trim_start_matches([' ', '='])
                    .trim()
                    .trim_matches(['"', '\'']);
                if umr_device::app::valid_package(v) {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

#[derive(Debug, Default)]
struct Shared {
    app_id: Option<String>,
    started: bool,
    stopped: Option<String>,
    replies: HashMap<u64, (Option<Value>, Option<Value>)>,
    problems: Vec<Problem>,
    eof: bool,
}

type State = Arc<(Mutex<Shared>, Condvar)>;

fn problem_from_log(text: &str, root: &Path, sink: &Sink, st: &mut Shared) {
    for line in text.lines() {
        if let Some(p) = parse_line(line, root) {
            if !st.problems.contains(&p) {
                sink(Event::Problem(p.clone()));
                st.problems.push(p);
            }
        }
    }
    if text.contains("EXCEPTION CAUGHT BY") {
        let p = Problem {
            severity: Severity::Error,
            message: text
                .lines()
                .find(|l| l.contains("EXCEPTION CAUGHT BY"))
                .unwrap_or(text)
                .trim_matches(|c: char| !c.is_alphanumeric() && c != ' ')
                .trim()
                .to_string(),
            file: None,
            line: None,
            column: None,
            source: "flutter".into(),
        };
        sink(Event::Problem(p.clone()));
        st.problems.push(p);
    }
}

/// Handles one decoded message (reader thread).
fn handle(msg: Message, root: &Path, sink: &Sink, st: &mut Shared) {
    match msg {
        Message::Event { name, params } => match name.as_str() {
            "app.start" => st.app_id = params["appId"].as_str().map(String::from),
            "app.started" => st.started = true,
            "app.progress" => {
                if let Some(m) = params["message"].as_str() {
                    let phase = if m.starts_with("Installing") {
                        Phase::Installing
                    } else {
                        Phase::Building
                    };
                    sink(Event::Phase {
                        phase,
                        message: m.to_string(),
                    });
                }
            }
            "app.log" | "daemon.logMessage" => {
                let text = params["log"]
                    .as_str()
                    .or_else(|| params["message"].as_str())
                    .unwrap_or_default();
                for l in text.lines() {
                    sink(Event::Log {
                        line: l.to_string(),
                    });
                }
                problem_from_log(text, root, sink, st);
            }
            "app.stop" => {
                let why = params["error"]
                    .as_str()
                    .unwrap_or("the app stopped")
                    .to_string();
                st.stopped = Some(why.clone());
                sink(Event::AppStopped { reason: why });
            }
            _ => {}
        },
        Message::Reply { id, result, error } => {
            st.replies.insert(id, (result, error));
        }
        Message::Text(t) => {
            if !t.trim().is_empty() {
                sink(Event::Log { line: t.clone() });
                problem_from_log(&t, root, sink, st);
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // owned by the reader thread
fn reader(out: ChildStdout, root: PathBuf, sink: Sink, state: State) {
    let r = BufReader::new(out);
    for line in r.lines() {
        let Ok(line) = line else { break };
        let (lock, cv) = &*state;
        let mut st = lock.lock().unwrap_or_else(PoisonError::into_inner);
        for m in decode(&line) {
            handle(m, &root, &sink, &mut st);
        }
        cv.notify_all();
    }
    let (lock, cv) = &*state;
    lock.lock().unwrap_or_else(PoisonError::into_inner).eof = true;
    cv.notify_all();
}

/// Waits until `done(state)` returns `Some`, the tool ends, or the timeout passes.
fn wait<T>(
    state: &State,
    timeout: Duration,
    mut done: impl FnMut(&mut Shared) -> Option<T>,
) -> Result<T, String> {
    let (lock, cv) = &**state;
    let start = Instant::now();
    let mut st = lock.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(v) = done(&mut st) {
            return Ok(v);
        }
        if st.eof {
            return Err(st
                .stopped
                .clone()
                .unwrap_or_else(|| "the Flutter tool exited".into()));
        }
        let left = timeout.checked_sub(start.elapsed()).ok_or("timed out")?;
        st = cv
            .wait_timeout(st, left.min(Duration::from_millis(500)))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

#[derive(Debug)]
struct Session {
    name: String,
    log: PathBuf,
    stdin: ChildStdin,
    state: State,
    next_id: u64,
}

fn launch_tool(ctx: &AdapterContext, sink: &Sink) -> Result<Session, AdapterError> {
    let flutter = ctx
        .flutter_root
        .as_ref()
        .map(|r| r.join("bin").join("flutter.bat"))
        .filter(|p| p.is_file())
        .ok_or_else(|| {
            AdapterError::Missing("Flutter SDK (run `umr install` for this project)".into())
        })?;
    let name = format!("flutter:{}", ctx.group);
    let log = ctx
        .log_dir
        .join(format!("flutter-{}.log", ctx.group.replace(':', "-")));
    let mut spec = ProcessSpec::new(&name, flutter, &log).args([
        "run",
        "--machine",
        "--debug",
        "-d",
        ctx.device.serial.as_str(),
    ]);
    spec.env.clone_from(&ctx.env);
    if let Some(j) = &ctx.jdk_home {
        spec.env.insert("JAVA_HOME".into(), j.display().to_string());
    }
    spec.env
        .insert("ANDROID_HOME".into(), ctx.sdk.display().to_string());
    spec.env
        .insert("ANDROID_SDK_ROOT".into(), ctx.sdk.display().to_string());
    spec.cwd = Some(ctx.project.clone());
    spec.group = Some(ctx.group.clone());
    sink(Event::Phase {
        phase: Phase::Building,
        message: format!("flutter run --machine -d {}", ctx.device.serial),
    });
    let (_pid, stdin, stdout) = ctx
        .supervisor
        .start_piped(spec)
        .map_err(|e| AdapterError::Failed(format!("cannot start Flutter: {e}")))?;
    let state: State = Arc::default();
    let (root, s2, st2) = (ctx.project.clone(), sink.clone(), state.clone());
    std::thread::Builder::new()
        .name("umr-flutter-reader".into())
        .spawn(move || reader(stdout, root, s2, st2))
        .map_err(|e| AdapterError::Failed(e.to_string()))?;
    Ok(Session {
        name,
        log,
        stdin,
        state,
        next_id: 1,
    })
}

/// Starts the tool and waits for `app.started`.
fn run_app(ctx: &AdapterContext, sink: &Sink) -> Result<Session, AdapterError> {
    let s = launch_tool(ctx, sink)?;
    let r = wait(&s.state, START_TIMEOUT, |st| st.started.then_some(()));
    if let Err(why) = r {
        let mut problems = s
            .state
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .problems
            .clone();
        ctx.supervisor.stop(&s.name);
        // the Android build writes to stderr (the tool's log): its summary and compiler lines
        let _ = ctx.supervisor.wait_exit(&s.name, Duration::from_secs(5));
        let text = std::fs::read_to_string(&s.log).unwrap_or_default();
        for p in text.lines().filter_map(|l| parse_line(l, &ctx.project)) {
            if !problems.contains(&p) {
                problems.push(p);
            }
        }
        let summary = umr_adapter::problems::gradle_failure(&text).unwrap_or(why);
        if problems.is_empty() {
            problems.push(Problem {
                severity: Severity::Error,
                message: summary.clone(),
                file: None,
                line: None,
                column: None,
                source: "gradle".into(),
            });
        }
        return Err(AdapterError::BuildFailed { summary, problems });
    }
    Ok(s)
}

impl Adapter for Flutter {
    fn id(&self) -> &'static str {
        "flutter"
    }
    fn display_name(&self) -> &'static str {
        "Flutter"
    }
    fn frameworks(&self) -> &'static [Framework] {
        &[Framework::Flutter]
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            hot_reload: true,
            hot_restart: true,
            incremental_build: true,
            structured_build_events: true,
        }
    }

    fn validate(&self, ctx: &AdapterContext) -> Vec<Issue> {
        let mut v = Vec::new();
        if !ctx.project.join("pubspec.yaml").is_file() {
            v.push(Issue {
                message: "no pubspec.yaml in the project folder".into(),
                fix: "open the Flutter project's root folder".into(),
            });
        }
        if !ctx.project.join("android").is_dir() {
            v.push(Issue {
                message: "the project has no android folder".into(),
                fix: "run `flutter create --platforms android .` in the project".into(),
            });
        }
        if !ctx
            .flutter_root
            .as_ref()
            .is_some_and(|r| r.join("bin").join("flutter.bat").is_file())
        {
            v.push(Issue {
                message: "Flutter SDK is not installed".into(),
                fix: "run `umr install` for this project".into(),
            });
        }
        if !ctx
            .jdk_home
            .as_ref()
            .is_some_and(|j| j.join("bin").join("java.exe").is_file())
        {
            v.push(Issue {
                message: "no JDK selected for the Android build".into(),
                fix: "run `umr install` for this project".into(),
            });
        }
        v
    }

    /// Dart code → hot reload; assets → hot restart; dependencies and native code → full.
    fn change_kind(&self, rel: &str) -> Option<ReloadKind> {
        if rel.starts_with("lib/")
            && std::path::Path::new(rel)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dart"))
        {
            Some(ReloadKind::Hot)
        } else if rel.starts_with("assets/") || rel.starts_with("fonts/") {
            Some(ReloadKind::Restart)
        } else if rel == "pubspec.yaml" || rel == "pubspec.lock" || rel.starts_with("android/") {
            Some(ReloadKind::Full)
        } else {
            None
        }
    }

    fn start(&self, ctx: &AdapterContext, sink: Sink) -> Result<Box<dyn RunningApp>, AdapterError> {
        if let Some(i) = self.validate(ctx).into_iter().next() {
            return Err(AdapterError::Missing(format!("{} ({})", i.message, i.fix)));
        }
        let package = application_id(&ctx.project).ok_or_else(|| {
            AdapterError::Missing("applicationId in android/app/build.gradle(.kts)".into())
        })?;
        let s = run_app(ctx, &sink)?;
        let pid = umr_device::app::pid(ctx.device.adb.port, &ctx.device.serial, &package);
        sink(Event::AppStarted {
            package: package.clone(),
            pid,
        });
        Ok(Box::new(FlutterApp {
            ctx: ctx.clone(),
            package,
            session: Some(s),
        }))
    }
}

#[derive(Debug)]
struct FlutterApp {
    ctx: AdapterContext,
    package: String,
    session: Option<Session>,
}

impl FlutterApp {
    fn send(&mut self, line: &str) -> Result<(), AdapterError> {
        let s = self
            .session
            .as_mut()
            .ok_or_else(|| AdapterError::Failed("the Flutter tool is not running".into()))?;
        writeln!(s.stdin, "{line}")
            .and_then(|()| s.stdin.flush())
            .map_err(|e| AdapterError::Failed(format!("Flutter tool: {e}")))
    }

    fn end_tool(&mut self) {
        if let Some(mut s) = self.session.take() {
            let app_id = s
                .state
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .app_id
                .clone();
            if let Some(a) = app_id {
                let id = s.next_id;
                let _ = writeln!(s.stdin, "{}", stop_request(id, &a));
                let _ = s.stdin.flush();
                let _ = wait(&s.state, Duration::from_secs(10), |st| {
                    st.replies.contains_key(&id).then_some(())
                });
            }
            drop(s.stdin);
            let _ = self
                .ctx
                .supervisor
                .wait_exit(&s.name, Duration::from_secs(10));
            self.ctx.supervisor.stop(&s.name);
        }
    }
}

impl RunningApp for FlutterApp {
    fn package(&self) -> &str {
        &self.package
    }

    fn reload(&mut self, kind: ReloadKind, sink: &Sink) -> Result<Duration, AdapterError> {
        let start = Instant::now();
        if kind == ReloadKind::Full {
            sink(Event::Phase {
                phase: Phase::Reloading,
                message: "full rebuild (flutter run again)".into(),
            });
            self.end_tool();
            self.session = Some(run_app(&self.ctx, sink)?);
        } else {
            let full = kind == ReloadKind::Restart;
            let (id, app_id, state) = {
                let s = self.session.as_mut().ok_or_else(|| {
                    AdapterError::Failed("the Flutter tool is not running".into())
                })?;
                let app_id = s
                    .state
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .app_id
                    .clone()
                    .ok_or_else(|| AdapterError::Failed("no Flutter app id yet".into()))?;
                let id = s.next_id;
                s.next_id += 1;
                (id, app_id, s.state.clone())
            };
            // compile errors of a reload go to the tool's stderr (its log): remember where it ends
            let log = self.session.as_ref().map(|s| s.log.clone());
            let log_pos = log
                .as_ref()
                .and_then(|l| std::fs::metadata(l).ok())
                .map_or(0, |m| m.len());
            sink(Event::Phase {
                phase: Phase::Reloading,
                message: if full { "hot restart" } else { "hot reload" }.into(),
            });
            self.send(&restart_request(id, &app_id, full))?;
            let (result, error) = wait(&state, RELOAD_TIMEOUT, |st| st.replies.remove(&id))
                .map_err(AdapterError::Failed)?;
            if let Some(e) = error {
                return Err(AdapterError::Failed(format!("Flutter: {e}")));
            }
            let r = result.unwrap_or(Value::Null);
            let code = r["code"].as_i64().unwrap_or(0);
            let message = r["message"].as_str().unwrap_or_default().to_string();
            if code != 0 {
                // give the tool a moment to flush stderr, then read what the reload added
                std::thread::sleep(Duration::from_millis(200));
                let added = log
                    .as_ref()
                    .and_then(|l| std::fs::read(l).ok())
                    .map(|b| {
                        let from = usize::try_from(log_pos).unwrap_or(0).min(b.len());
                        String::from_utf8_lossy(&b[from..]).into_owned()
                    })
                    .unwrap_or_default();
                let mut problems: Vec<Problem> = Vec::new();
                for p in message
                    .lines()
                    .chain(added.lines())
                    .filter_map(|l| parse_line(l, &self.ctx.project))
                {
                    if !problems.contains(&p) {
                        problems.push(p);
                    }
                }
                if problems.is_empty() {
                    problems.push(Problem {
                        severity: Severity::Error,
                        message: message.clone(),
                        file: None,
                        line: None,
                        column: None,
                        source: "flutter".into(),
                    });
                }
                for p in &problems {
                    sink(Event::Problem(p.clone()));
                }
                return Err(AdapterError::BuildFailed {
                    summary: message,
                    problems,
                });
            }
            sink(Event::Log { line: message });
        }
        let took = start.elapsed();
        sink(Event::Reloaded {
            kind,
            ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        });
        Ok(took)
    }

    fn is_alive(&mut self) -> bool {
        let tool = self
            .session
            .as_ref()
            .is_some_and(|s| self.ctx.supervisor.is_running(&s.name));
        tool && umr_device::app::pid(
            self.ctx.device.adb.port,
            &self.ctx.device.serial,
            &self.package,
        )
        .is_some()
    }

    fn stop(mut self: Box<Self>) {
        self.end_tool();
        let _ = umr_device::app::force_stop(
            self.ctx.device.adb.port,
            &self.ctx.device.serial,
            &self.package,
        );
        self.ctx.supervisor.stop_group(&self.ctx.group);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_machine_protocol_lines() {
        let m = decode(
            r#"[{"event":"app.start","params":{"appId":"a1","deviceId":"emulator-5554","supportsRestart":true}}]"#,
        );
        assert!(
            matches!(&m[0], Message::Event { name, params } if name == "app.start" && params["appId"] == "a1")
        );
        let r = decode(
            r#"[{"id":3,"result":{"code":0,"message":"Reloaded 1 of 640 libraries in 312ms."}}]"#,
        );
        assert!(matches!(&r[0], Message::Reply { id: 3, result: Some(v), .. } if v["code"] == 0));
        assert_eq!(
            decode("Running Gradle task 'assembleDebug'..."),
            vec![Message::Text(
                "Running Gradle task 'assembleDebug'...".into()
            )]
        );
        assert!(matches!(&decode("[not json")[0], Message::Text(_)));
    }

    #[test]
    fn requests_have_the_protocol_shape() {
        let r: Value = serde_json::from_str(&restart_request(7, "a1", false)).unwrap();
        assert_eq!(r[0]["method"], "app.restart");
        assert_eq!(r[0]["params"]["fullRestart"], false);
        assert_eq!(r[0]["id"], 7);
        let s: Value = serde_json::from_str(&stop_request(8, "a1")).unwrap();
        assert_eq!(s[0]["method"], "app.stop");
    }

    #[test]
    fn events_drive_state_and_problems() {
        let root = Path::new(r"C:\p");
        let seen: Arc<Mutex<Vec<Event>>> = Arc::default();
        let s2 = seen.clone();
        let sink: Sink = Arc::new(move |e| s2.lock().unwrap().push(e));
        let mut st = Shared::default();
        for line in [
            r#"[{"event":"app.start","params":{"appId":"a1"}}]"#,
            r#"[{"event":"app.progress","params":{"message":"Running Gradle task 'assembleDebug'..."}}]"#,
            "lib/main.dart:12:5: Error: Expected ';' after this.",
            r#"[{"event":"app.started","params":{"appId":"a1"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"══╡ EXCEPTION CAUGHT BY WIDGETS LIBRARY ╞══"}}]"#,
            r#"[{"id":1,"result":{"code":0,"message":"ok"}}]"#,
            r#"[{"event":"app.stop","params":{"appId":"a1"}}]"#,
        ] {
            for m in decode(line) {
                handle(m, root, &sink, &mut st);
            }
        }
        assert_eq!(st.app_id.as_deref(), Some("a1"));
        assert!(st.started);
        assert_eq!(st.problems.len(), 2, "{:?}", st.problems);
        assert_eq!(st.problems[0].file.as_deref(), Some("lib/main.dart"));
        assert!(st.problems[1]
            .message
            .contains("EXCEPTION CAUGHT BY WIDGETS LIBRARY"));
        assert!(st.replies.contains_key(&1));
        assert!(st.stopped.is_some());
        let ev = seen.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(
            e,
            Event::Phase {
                phase: Phase::Building,
                ..
            }
        )));
        assert!(ev.iter().any(|e| matches!(e, Event::AppStopped { .. })));
    }

    #[test]
    fn change_kinds() {
        let f = Flutter;
        assert_eq!(f.change_kind("lib/src/page.dart"), Some(ReloadKind::Hot));
        assert_eq!(f.change_kind("assets/logo.png"), Some(ReloadKind::Restart));
        assert_eq!(f.change_kind("pubspec.yaml"), Some(ReloadKind::Full));
        assert_eq!(
            f.change_kind("android/app/build.gradle.kts"),
            Some(ReloadKind::Full)
        );
        assert_eq!(f.change_kind("test/widget_test.dart"), None);
    }

    #[test]
    fn application_id_from_gradle() {
        let d = std::env::temp_dir().join(format!("umr-flutter-appid-{}", std::process::id()));
        std::fs::create_dir_all(d.join("android").join("app")).unwrap();
        std::fs::write(
            d.join("android").join("app").join("build.gradle.kts"),
            "android {\n    defaultConfig {\n        applicationId = \"com.umr.flutterdemo\"\n    }\n}\n",
        )
        .unwrap();
        assert_eq!(application_id(&d).as_deref(), Some("com.umr.flutterdemo"));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(
            umr_adapter::contract::check_static(&Flutter),
            Vec::<String>::new()
        );
    }
}
