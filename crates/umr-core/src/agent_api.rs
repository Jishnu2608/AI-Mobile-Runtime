//! Agent support (Phase 6): the runtime digest (MCP-002), the permission gate with user
//! approvals and grants (MCP-003), and the audit log of agent calls (AUTH-001).
//!
//! Permission model for non-admin clients (agents):
//! * scopes decide which methods a client may call at all (`observe`, `interact`, `control`;
//!   agents never get `admin`);
//! * some `control` calls also need the user's consent once: running or checking a project
//!   folder the client has no `project:<folder>` grant for, and switching / booting the device
//!   (`device:switch`). The call waits (up to [`APPROVAL_TIMEOUT`]) while the user answers in
//!   the preview window or with `umr approvals`; "always" records the grant;
//! * every call a non-admin client makes is written to the audit log (sanitised parameters,
//!   outcome).
//!
//! Limit: an agent that can run shell commands as the same Windows user can also run `umr`
//! itself, which uses the admin token. Scopes and approvals govern the MCP channel; they are not
//! a sandbox against such an agent.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use umr_api::methods::{self, AuditParams, DecideParams, DigestParams, GrantsParams};
use umr_api::rpc::{codes, Id, Request, Response, RpcError};
use umr_diagnostics::sanitize;

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
            "no answer from the user within {} s for: {action} ({detail}). Ask them to approve it in the preview window or with `umr approvals`, then call again",
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
