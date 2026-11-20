//! The setup window's home screen for returning users (AUD-P3-24): recent projects, what runs
//! now, the current problems and the agent-fix feed, in one call (`home.summary`).
//!
//! Recent projects live in `<data>/recent-projects.json`, updated on every `project.run`; the
//! feed lives in the diagnostics store ([`crate::diag_api::DiagStore::note_check`]) and lasts as
//! long as the Core runs.

use std::path::Path;
use std::sync::PoisonError;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tumr_api::rpc::{Id, Response};

use crate::api_server::{ok, ApiState};

const FILE: &str = "recent-projects.json";
const KEEP: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: String,
    pub adapter: String,
    pub last_run_unix: u64,
    pub runs: u32,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[must_use]
pub fn load(data_root: &Path) -> Vec<RecentProject> {
    std::fs::read_to_string(data_root.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// `list` with `project` moved to the top (newest first, at most [`KEEP`]).
#[must_use]
pub fn with_run(
    mut list: Vec<RecentProject>,
    project: &str,
    adapter: &str,
    at: u64,
) -> Vec<RecentProject> {
    let key = |p: &str| {
        p.trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    let runs = list
        .iter()
        .find(|r| key(&r.path) == key(project))
        .map_or(0, |r| r.runs);
    list.retain(|r| key(&r.path) != key(project));
    list.insert(
        0,
        RecentProject {
            path: project.to_string(),
            adapter: adapter.to_string(),
            last_run_unix: at,
            runs: runs + 1,
        },
    );
    list.truncate(KEEP);
    list
}

/// Called when a project starts running.
pub fn record_run(data_root: &Path, project: &str, adapter: &str) {
    let list = with_run(load(data_root), project, adapter, now_unix());
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(data_root.join(FILE), text);
    }
}

/// `home.summary`.
pub fn summary(state: &ApiState, id: Id) -> Response {
    let recent: Vec<Value> = load(&state.data_root)
        .into_iter()
        .map(|r| {
            let exists = Path::new(&r.path).is_dir();
            json!({ "path": r.path, "adapter": r.adapter, "last_run_unix": r.last_run_unix,
                    "runs": r.runs, "exists": exists })
        })
        .collect();
    let session = crate::session_api::current(state);
    let run = state
        .run_info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let d = state.diag.lock().unwrap_or_else(PoisonError::into_inner);
    let mut counts = serde_json::Map::new();
    for e in d.problems.entries() {
        let c = serde_json::to_value(e.diagnostic.category)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let n = counts.get(&c).and_then(Value::as_u64).unwrap_or(0) + 1;
        counts.insert(c, json!(n));
    }
    let feed: Vec<Value> = d
        .feed
        .iter()
        .rev()
        .take(20)
        .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
        .collect();
    let since_day = crate::diag_api::now_ms().saturating_sub(24 * 3600 * 1000);
    let fixed_today = d
        .feed
        .iter()
        .filter(|e| e.kind == "fixed" && e.at_ms >= since_day)
        .count();
    ok(
        id,
        &json!({
            "recent": recent,
            "session": session,
            "run": run,
            "problems": { "total": d.problems.len(), "by_category": counts },
            "feed": feed,
            "fixed_today": fixed_today,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_moves_its_project_to_the_top_and_counts_it() {
        let l = with_run(Vec::new(), r"D:\a", "flutter", 1);
        let l = with_run(l, r"D:\b", "react-native", 2);
        let l = with_run(l, "d:/a/", "flutter", 3);
        assert_eq!(
            l.len(),
            2,
            "the same folder, written differently, is one project"
        );
        assert_eq!(
            (l[0].path.as_str(), l[0].runs, l[0].last_run_unix),
            ("d:/a/", 2, 3)
        );
        assert_eq!(l[1].path, r"D:\b");
        let mut many = Vec::new();
        for i in 0..20 {
            many = with_run(many, &format!(r"D:\p{i}"), "x", i);
        }
        assert_eq!(many.len(), KEEP);
        assert_eq!(many[0].path, r"D:\p19");
    }
}
