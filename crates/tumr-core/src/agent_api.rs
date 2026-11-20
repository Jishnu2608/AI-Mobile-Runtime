//! Agent support (Phase 6): the runtime digest (MCP-002), the permission gate with user
//! approvals and grants (MCP-003), and the audit log of agent calls (AUTH-001).
//!
//! Permission model for non-admin clients (agents):
//! * scopes decide which methods a client may call at all (`observe`, `interact`, `control`;
//!   agents never get `admin`);
//! * some `control` calls also need the user's consent once: running or checking a project
//!   folder the client has no `project:<folder>` grant for, and switching / booting the device
//!   (`device:switch`). The call waits (up to [`APPROVAL_TIMEOUT`]) while the user answers in
//!   the preview window or with `tumr approvals`; "always" records the grant;
//! * every call a non-admin client makes is written to the audit log (sanitised parameters,
//!   outcome).
//!
//! Limit: an agent that can run shell commands as the same Windows user can also run `tumr`
//! itself, which uses the admin token. Scopes and approvals govern the MCP channel; they are not
//! a sandbox against such an agent.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tumr_api::methods::{
    self, AgentCheckParams, AuditParams, DecideParams, DigestParams, GrantsParams,
};
use tumr_api::rpc::{codes, Id, Request, Response, RpcError};
use tumr_diagnostics::{sanitize, Category, Origin};

use crate::api_server::{ok, ApiState};
use crate::auth::{Principal, Scope};
use crate::diag_api::now_ms;

/// How long a gated call waits for the user.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
struct Pending {
    id: String,
    client_id: String,
    client: String,
    action: String,
    detail: String,
    grant: String,
    created_ms: u64,
    decision: Option<(bool, bool)>,
}

/// Requests waiting for the user's answer.
#[derive(Debug, Default)]
pub struct Approvals {
    pending: Mutex<Vec<Pending>>,
    changed: Condvar,
    next: Mutex<u64>,
}

/// What the user answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Allow { always: bool },
    Deny,
    NoAnswer,
}

impl Approvals {
    /// Queues a request and waits for the user (or `timeout`).
    pub fn ask(
        &self,
        p: &Principal,
        action: &str,
        detail: &str,
        grant: &str,
        timeout: Duration,
    ) -> Answer {
        let id = {
            let mut n = self.next.lock().unwrap_or_else(PoisonError::into_inner);
            *n += 1;
            format!("a{}", *n)
        };
        tracing::info!(client = %p.client_id, %action, %detail, "waiting for the user's approval {id}");
        let mut list = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        list.push(Pending {
            id: id.clone(),
            client_id: p.client_id.clone(),
            client: p.name.clone(),
            action: action.to_string(),
            detail: detail.to_string(),
            grant: grant.to_string(),
            created_ms: now_ms(),
            decision: None,
        });
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(i) = list.iter().position(|e| e.id == id) {
                if let Some((allow, always)) = list[i].decision {
                    list.remove(i);
                    return if allow {
                        Answer::Allow { always }
                    } else {
                        Answer::Deny
                    };
                }
            } else {
                return Answer::Deny;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                list.retain(|e| e.id != id);
                return Answer::NoAnswer;
            }
            list = self
                .changed
                .wait_timeout(list, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Records the user's answer; `None` if there is no such request.
    pub fn decide(&self, id: &str, allow: bool, always: bool) -> Option<(String, String)> {
        let mut list = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let e = list
            .iter_mut()
            .find(|e| e.id == id && e.decision.is_none())?;
        e.decision = Some((allow, always));
        let out = (e.client_id.clone(), e.grant.clone());
        drop(list);
        self.changed.notify_all();
        Some(out)
    }

    #[must_use]
    pub fn list(&self) -> Vec<Value> {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|e| e.decision.is_none())
            .map(|e| {
                json!({ "id": e.id, "client": e.client, "client_id": e.client_id,
                    "action": e.action, "detail": e.detail, "created_ms": e.created_ms })
            })
            .collect()
    }
}

/// Lower-case, `/`-separated folder for comparisons.
fn folder_key(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Whether `grants` cover `grant` (a `project:` grant also covers sub-folders).
#[must_use]
pub fn covered(grants: &[String], grant: &str) -> bool {
    grants.iter().any(|g| {
        g == grant
            || match (g.strip_prefix("project:"), grant.strip_prefix("project:")) {
                (Some(have), Some(want)) => want == have || want.starts_with(&format!("{have}/")),
                _ => false,
            }
    })
}

/// What a non-admin client's call needs beyond its scope: `(action, detail, grant)`.
fn needs_consent(method: &str, params: &Value) -> Option<(String, String, String)> {
    match method {
        methods::PROJECT_RUN | methods::ADAPTER_CHECK => {
            let raw = params["path"].as_str().unwrap_or("");
            let path = crate::run_api::normalise(raw);
            Some((
                if method == methods::PROJECT_RUN {
                    "run project"
                } else {
                    "check adapter on project"
                }
                .into(),
                path.clone(),
                format!("project:{}", folder_key(&path)),
            ))
        }
        methods::SESSION_START => Some((
            "switch / boot device".into(),
            format!(
                "profile {}",
                params["profile"].as_str().unwrap_or("standard")
            ),
            "device:switch".into(),
        )),
        _ => None,
    }
}

/// MCP-003: lets the call through, or asks the user first. `None` = go ahead.
pub fn gate(state: &ApiState, p: Option<&Principal>, req: &Request) -> Option<Response> {
    gate_with(state, p, req, APPROVAL_TIMEOUT)
}

pub(crate) fn gate_with(
    state: &ApiState,
    p: Option<&Principal>,
    req: &Request,
    timeout: Duration,
) -> Option<Response> {
    let p = p?;
    if p.allows(Scope::Admin) {
        return None;
    }
    let (action, detail, grant) = needs_consent(&req.method, &req.params)?;
    let grants = state
        .auth
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .grants(&p.client_id);
    if covered(&grants, &grant) {
        return None;
    }
    let denied = |msg: String| {
        Some(Response::err(
            Some(req.id.clone()),
            RpcError::new(codes::FORBIDDEN, msg),
        ))
    };
    match state.approvals.ask(p, &action, &detail, &grant, timeout) {
        Answer::Allow { always } => {
            if always {
                let _ = state
                    .auth
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .add_grant(&p.client_id, &grant);
            }
            None
        }
        Answer::Deny => denied(format!("the user declined: {action} ({detail})")),
        Answer::NoAnswer => denied(format!(
            "no answer from the user within {} s for: {action} ({detail}). Ask them to approve it in the preview window or with `tumr approvals`, then call again",
            timeout.as_secs()
        )),
    }
}

/// AUTH-001: one audit row per call of a non-admin client.
pub fn audit(state: &ApiState, p: Option<&Principal>, req: &Request, resp: &Response) {
    let Some(p) = p.filter(|p| !p.allows(Scope::Admin)) else {
        return;
    };
    let mut params = sanitize(&req.params.to_string());
    if params.len() > 300 {
        let cut = (0..=300)
            .rev()
            .find(|i| params.is_char_boundary(*i))
            .unwrap_or(0);
        params.truncate(cut);
        params.push('…');
    }
    let outcome = match &resp.error {
        None => "ok".to_string(),
        Some(e) => format!("error {}: {}", e.code, sanitize(&e.message)),
    };
    let at = i64::try_from(now_ms() / 1000).unwrap_or(0);
    let db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
    if let Err(e) = db.audit(
        at,
        Some(&p.client_id),
        &req.method,
        Some(&format!("{params} → {outcome}")),
    ) {
        tracing::warn!(error = %e, "audit log write failed");
    }
}

pub fn approvals_list(state: &ApiState, id: Id) -> Response {
    ok(id, &json!({ "pending": state.approvals.list() }))
}

pub fn approvals_decide(state: &ApiState, id: Id, p: &DecideParams) -> Response {
    let allow = match p.decision.as_str() {
        "allow" => true,
        "deny" => false,
        other => {
            return Response::err(
                Some(id),
                RpcError::new(
                    codes::INVALID_PARAMS,
                    format!("decision is allow or deny, not '{other}'"),
                ),
            )
        }
    };
    match state.approvals.decide(&p.id, allow, p.always && allow) {
        Some(_) => ok(
            id,
            &json!({ "id": p.id, "decision": p.decision, "always": p.always && allow }),
        ),
        None => Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                format!("no pending request {}", p.id),
            ),
        ),
    }
}

pub fn audit_list(state: &ApiState, id: Id, p: &AuditParams) -> Response {
    let db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
    match db.audit_recent(p.client.as_deref(), p.limit.unwrap_or(50).clamp(1, 1000)) {
        Ok(rows) => ok(
            id,
            &json!({ "entries": rows.into_iter().map(|(at, c, a, d)| json!({
                "at_unix": at, "client_id": c, "action": a, "detail": d })).collect::<Vec<_>>() }),
        ),
        Err(e) => Response::err(
            Some(id),
            RpcError::new(codes::INTERNAL_ERROR, e.to_string()),
        ),
    }
}

pub fn set_grants(state: &ApiState, id: Id, p: &GrantsParams) -> Response {
    let grants: Vec<String> = p
        .grants
        .iter()
        .map(|g| match g.strip_prefix("project:") {
            Some(path) => format!("project:{}", folder_key(&crate::run_api::normalise(path))),
            None => g.clone(),
        })
        .collect();
    if let Some(bad) = grants
        .iter()
        .find(|g| !g.starts_with("project:") && g.as_str() != "device:switch")
    {
        return Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                format!("unknown grant '{bad}' (project:<folder> or device:switch)"),
            ),
        );
    }
    let mut auth = state.auth.lock().unwrap_or_else(PoisonError::into_inner);
    match auth.set_grants(&p.id, grants) {
        Ok(true) => ok(id, &json!({ "id": p.id, "grants": auth.grants(&p.id) })),
        Ok(false) => Response::err(
            Some(id),
            RpcError::new(codes::INVALID_PARAMS, format!("no client {}", p.id)),
        ),
        Err(e) => Response::err(
            Some(id),
            RpcError::new(codes::INTERNAL_ERROR, e.to_string()),
        ),
    }
}

fn short(s: &str, n: usize) -> String {
    let s = sanitize(s.lines().next().unwrap_or(""));
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// This Core's start time (ms): problem numbers (cursors) are only valid within one start.
fn core_epoch() -> u64 {
    static EPOCH: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(crate::diag_api::now_ms)
}

/// A folder for comparison: whatever the case, slashes or `\\?\` prefix.
fn norm_folder(s: &str) -> String {
    let p = std::path::Path::new(s);
    let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    c.display()
        .to_string()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// AUD-P1-8: the agent works on the running project when its folder is the project, contains it
/// (an agent opened at a repository root with the app in `apps/mobile`) or lies inside it (an
/// agent opened in `lib/`). Before, only the exact folder counted and the stop hook stayed
/// silent for the other two.
fn works_on(project: &str, agent: &str) -> bool {
    let (p, a) = (norm_folder(project), norm_folder(agent));
    let inside = |child: &str, parent: &str| {
        child.len() > parent.len()
            && child.starts_with(parent)
            && child.as_bytes()[parent.len()] == b'\\'
    };
    !p.is_empty() && !a.is_empty() && (p == a || inside(&p, &a) || inside(&a, &p))
}

/// The problems an agent's stop hook reports: open build problems, and runtime / crash problems
/// after `since`; never the environment's (the agent cannot fix those in the code).
fn problems_for_agent(d: &crate::diag_api::DiagStore, since: u64) -> Vec<Value> {
    let mut picked: Vec<_> = d
        .problems
        .entries()
        .iter()
        .filter(|e| {
            let g = &e.diagnostic;
            // AUD-P2-25: from before a build that is still running: not reported until it ends
            g.origin != Origin::Environment
                && !d.is_checking(&g.fingerprint)
                && match g.category {
                    Category::Build => true,
                    // new since the last report, or sent by the user ("Send to agent")
                    Category::Runtime | Category::Crash => {
                        e.seq > since || d.flagged.contains(&g.fingerprint)
                    }
                    _ => d.flagged.contains(&g.fingerprint),
                }
        })
        .collect();
    picked.sort_by_key(|e| std::cmp::Reverse(e.seq));
    picked
        .iter()
        .take(5)
        .map(|e| {
            let g = &e.diagnostic;
            let at = match (&g.file, g.line) {
                (Some(f), Some(l)) => Some(format!("{}:{l}", sanitize(f))),
                (Some(f), None) => Some(sanitize(f)),
                _ => None,
            };
            json!({ "fp": g.fingerprint, "category": g.category, "at": at,
                "msg": short(&sanitize(&g.message), 300), "count": e.count })
        })
        .collect()
}

/// `agent.check` (HOOK-001): waits for the watcher and the reload it started, then the problems
/// the agent should still fix.
pub fn check(state: &ApiState, id: Id, p: &AgentCheckParams) -> Response {
    use std::sync::atomic::Ordering;
    let running = state
        .run_info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|r| p.project.trim().is_empty() || works_on(&r.project, &p.project))
        // PLUG-003: a run that did not build yet counts: its build errors are for the agent
        || state
            .run_retry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|r| p.project.trim().is_empty() || works_on(&r.project, &p.project));
    if !running {
        return ok(id, &json!({ "status": "not_running", "problems": [] }));
    }
    // the conditional timer: nothing building → answer after the debounce; a build or reload
    // running → wait for it however long it takes, up to the caller's limit
    let start = Instant::now();
    let deadline =
        start + Duration::from_millis(p.timeout_ms.min(tumr_api::methods::AGENT_CHECK_MAX_MS));
    // the watcher picks up the last save after its debounce
    if p.timeout_ms > 0 {
        std::thread::sleep(crate::watch::DEBOUNCE + Duration::from_millis(500));
    }
    loop {
        let (watch, session, app) = (
            state.watch_busy.load(Ordering::SeqCst),
            state.session_busy.load(Ordering::SeqCst),
            state.run_app.try_lock().is_err(),
        );
        if !watch && !session && !app {
            break;
        }
        if Instant::now() >= deadline {
            let what = if session {
                "the device"
            } else {
                "a build or reload"
            };
            return ok(
                id,
                &json!({ "status": "busy", "problems": [], "running": what,
                         "waited_s": start.elapsed().as_secs() }),
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let alive = state
        .run_app
        .try_lock()
        .ok()
        .and_then(|mut g| g.as_mut().map(|a| a.is_alive()));
    let mut d = state.diag.lock().unwrap_or_else(PoisonError::into_inner);
    // a cursor from an earlier Core start (or ahead of this one's numbers) means nothing here
    let since = if p.epoch.is_some_and(|e| e != core_epoch()) || p.since > d.problems.cursor() {
        0
    } else {
        p.since
    };
    let problems = problems_for_agent(&d, since);
    // AUD-P1-7: the user sent these ("Send to agent"): the hook reports them even when its loop
    // guard would let the agent stop (the same problem was reported before)
    let flagged: Vec<String> = problems
        .iter()
        .filter_map(|v| v["fp"].as_str())
        .filter(|fp| d.flagged.contains(*fp))
        .map(String::from)
        .collect();
    // AUD-P3-24: the home screen's feed of what the agent was told and what it fixed
    let reported: Vec<(String, String)> = problems
        .iter()
        .map(|v| {
            (
                v["fp"].as_str().unwrap_or_default().to_string(),
                v["msg"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    if !p.probe {
        d.note_check(&reported);
    }
    ok(
        id,
        &json!({ "status": "ok", "problems": problems, "flagged": flagged, "cursor": d.problems.cursor(), "epoch": core_epoch(), "alive": alive,
                 "waited_s": start.elapsed().as_secs() }),
    )
}

/// `agent.note`: a `connected` or `gave_up` line in the home screen's agent feed.
pub fn note(state: &ApiState, id: Id, p: &tumr_api::methods::AgentNoteParams) -> Response {
    let kind = match p.kind.as_str() {
        "connected" => "connected",
        "gave_up" => "gave_up",
        other => {
            return Response::err(
                Some(id),
                RpcError::new(
                    codes::INVALID_PARAMS,
                    format!("unknown note kind '{other}'"),
                ),
            )
        }
    };
    let message = short(&sanitize(&p.message), 300);
    state
        .diag
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .note(kind, message);
    ok(id, &json!({}))
}

/// MCP-002: what changed, small enough to call after every edit (≤ 1 KB).
pub fn digest(state: &ApiState, id: Id, p: &DigestParams) -> Response {
    ok(id, &digest_value(state, p.since.as_deref()))
}

pub(crate) fn digest_value(state: &ApiState, since: Option<&str>) -> Value {
    let (dsince, esince) = since
        .and_then(|s| s.split_once('.'))
        .map_or((0, 0), |(a, b)| {
            (a.parse().unwrap_or(0), b.parse().unwrap_or(0))
        });
    let session = crate::session_api::current(state);
    let run = state
        .run_info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let alive = state
        .run_app
        .try_lock()
        .ok()
        .and_then(|mut g| g.as_mut().map(|a| a.is_alive()));
    let (events, ecursor) = state
        .run_events
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .since(esince, 50);
    let last = events.iter().rev().find_map(|e| {
        let text = match e["event"].as_str()? {
            "phase" => format!("{}: {}", e["phase"].as_str()?, e["message"].as_str()?),
            "app_started" => "app started".into(),
            "reloaded" => format!("reloaded ({}) in {} ms", e["kind"].as_str()?, e["ms"]),
            "app_stopped" => format!("app stopped: {}", e["reason"].as_str()?),
            _ => return None,
        };
        Some(short(&text, 100))
    });
    let d = state.diag.lock().unwrap_or_else(PoisonError::into_inner);
    let mut counts = serde_json::Map::new();
    for e in d.problems.entries() {
        let k = serde_json::to_value(e.diagnostic.category)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let n = counts.get(&k).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(k, json!(n + 1));
    }
    let mut new: Vec<_> = d.problems.since(dsince).collect();
    new.sort_by_key(|e| std::cmp::Reverse(e.seq));
    let n_new = new.len();
    let top: Vec<Value> = new
        .iter()
        .take(3)
        .map(|e| {
            let g = &e.diagnostic;
            let at = match (&g.file, g.line) {
                (Some(f), Some(l)) => Some(format!("{}:{l}", sanitize(f))),
                (Some(f), None) => Some(sanitize(f)),
                _ => None,
            };
            json!({ "fp": g.fingerprint, "category": g.category, "origin": g.origin,
                "at": at, "msg": short(&g.message, 120), "count": e.count })
        })
        .collect();
    let cursor = format!("{}.{}", d.problems.cursor(), ecursor);
    let total = d.problems.len();
    drop(d);
    let hint = if run.is_none() {
        "nothing runs: use run_project"
    } else if alive == Some(false) {
        "the app is not running: see get_diagnostics (crash?) then reload or run_project"
    } else if n_new > 0 {
        "new problems: get_diagnostic <fp> for details"
    } else {
        "no new problems"
    };
    json!({
        "session": session.map(|s| json!({ "state": s.state, "profile": s.profile, "device": s.serial, "app": s.app })),
        "run": run.map(|r| json!({ "adapter": r.adapter, "package": r.package, "alive": alive })),
        "problems": { "total": total, "by_category": counts, "new": n_new, "top": top },
        "last_event": last,
        "cursor": cursor,
        "hint": hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(cat: Category, origin: Origin, msg: &str) -> tumr_diagnostics::Diagnostic {
        let mut d = tumr_diagnostics::from_message("kotlinc", msg, cat);
        d.category = cat;
        d.origin = origin;
        d.fingerprint = format!("fp-{msg}");
        d
    }

    #[test]
    fn hook_gets_open_build_problems_and_new_crashes_only() {
        let mut d = crate::diag_api::DiagStore::default();
        d.problem(problem(
            Category::Build,
            Origin::App,
            "Unresolved reference: foo",
        ));
        d.problem(problem(
            Category::Environment,
            Origin::Environment,
            "SDK missing",
        ));
        d.problem(problem(
            Category::Crash,
            Origin::App,
            "NullPointerException",
        ));
        let all = problems_for_agent(&d, 0);
        let msgs: Vec<_> = all.iter().map(|v| v["msg"].as_str().unwrap()).collect();
        assert_eq!(msgs, ["NullPointerException", "Unresolved reference: foo"]);
        // after the cursor: the crash was already reported, the build error is still open
        let later = problems_for_agent(&d, d.problems.cursor());
        assert_eq!(later.len(), 1);
        assert_eq!(later[0]["category"], "build");
        d.build_fixed();
        assert_eq!(
            problems_for_agent(&d, d.problems.cursor()),
            Vec::<Value>::new()
        );
    }

    #[test]
    fn the_agent_works_on_a_project_in_or_around_its_folder() {
        let app = r"C:\Repo\apps\mobile";
        assert!(works_on(app, r"C:\Repo\apps\mobile"), "same folder");
        assert!(works_on(app, "c:/repo/"), "agent at the repository root");
        assert!(
            works_on(app, r"C:\Repo\apps\mobile\lib"),
            "agent inside the project"
        );
        assert!(!works_on(app, r"C:\Repo\apps\web"), "a sibling project");
        assert!(
            !works_on(app, r"C:\Repo\apps\mob"),
            "a prefix that is not a folder"
        );
        assert!(!works_on(app, ""), "no folder");
        assert!(
            works_on(r"C:\Proj\App\", "c:/proj/app"),
            "case and slash blind"
        );
    }

    fn principal() -> Principal {
        Principal {
            client_id: "c_1".into(),
            name: "claude-code".into(),
            scopes: vec![Scope::Observe, Scope::Control],
        }
    }

    #[test]
    fn project_grants_cover_sub_folders_only() {
        let g = vec![
            "project:d:/work/app".to_string(),
            "device:switch".to_string(),
        ];
        assert!(covered(&g, "project:d:/work/app"));
        assert!(covered(&g, "project:d:/work/app/sub"));
        assert!(!covered(&g, "project:d:/work/application"));
        assert!(!covered(&g, "project:d:/work"));
        assert!(covered(&g, "device:switch"));
        assert!(!covered(&[], "device:switch"));
    }

    #[test]
    fn approvals_allow_deny_and_time_out() {
        let a = std::sync::Arc::new(Approvals::default());
        let p = principal();
        // nobody answers
        assert_eq!(
            a.ask(&p, "x", "y", "g", Duration::from_millis(50)),
            Answer::NoAnswer
        );
        assert!(a.list().is_empty(), "a timed-out request leaves the queue");
        // the user allows "always" from another thread
        let a2 = a.clone();
        let t = std::thread::spawn(move || loop {
            if let Some(id) = a2
                .list()
                .first()
                .and_then(|v| v["id"].as_str().map(String::from))
            {
                assert!(a2.decide(&id, true, true).is_some());
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        });
        assert_eq!(
            a.ask(
                &p,
                "run project",
                "D:\\app",
                "project:d:/app",
                Duration::from_secs(5)
            ),
            Answer::Allow { always: true }
        );
        t.join().unwrap();
        // and denies
        let a3 = a.clone();
        let t = std::thread::spawn(move || loop {
            if let Some(id) = a3
                .list()
                .first()
                .and_then(|v| v["id"].as_str().map(String::from))
            {
                a3.decide(&id, false, false);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        });
        assert_eq!(
            a.ask(&p, "x", "y", "g", Duration::from_secs(5)),
            Answer::Deny
        );
        t.join().unwrap();
        assert!(a.decide("a999", true, false).is_none());
    }

    #[test]
    fn only_run_check_and_device_switch_need_consent() {
        assert!(needs_consent(methods::PROJECT_RUN, &json!({"path": "C:\\x"})).is_some());
        assert!(needs_consent(methods::SESSION_START, &json!({"profile": "large"})).is_some());
        assert!(needs_consent(methods::PROJECT_RELOAD, &json!({})).is_none());
        assert!(needs_consent(methods::DEVICE_INPUT, &json!({})).is_none());
    }
}
