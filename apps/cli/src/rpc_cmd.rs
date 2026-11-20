//! VSC-001: `tumr rpc --stdio`, the VS Code extension's line to the Runtime Core.
//!
//! The extension starts this once and talks newline-delimited JSON on stdin / stdout:
//! `{"id": 1, "method": "preview.open", "params": {…}}` → `{"id": 1, "result": …}` or
//! `{"id": 1, "error": "…"}`. Requests run concurrently (each on its own Core connection), so a
//! slow call (a rebuild, the lock-screen check) does not hold up the others. Only the methods in
//! [`ALLOWED`] pass, the same set the desktop window may use plus stopping a run, so a bug in the
//! extension's web view cannot reach installs, trust or agent administration.

use std::process::ExitCode;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tumr_api::methods;

use crate::core_process::CoreTarget;

/// API methods the VS Code extension may call.
pub const ALLOWED: &[&str] = &[
    methods::INFO,
    methods::PREVIEW_OPEN,
    methods::PREVIEW_STATUS,
    methods::PREVIEW_SELFTEST,
    methods::DEVICE_ROTATE,
    methods::DEVICE_FINGERPRINT,
    methods::DEVICE_AUTH_STATE,
    methods::DEVICE_POSTURE,
    methods::DEVICE_SCREENSHOT,
    methods::DEVICE_PROFILES,
    methods::SESSION_STATUS,
    methods::SESSION_START,
    methods::PROJECT_STATUS,
    methods::PROJECT_RELOAD,
    methods::PROJECT_STOP,
    methods::PROJECT_DETECT,
    methods::DIAGNOSTICS_LIST,
    methods::DIAGNOSTICS_GET,
    methods::LOGS_TAIL,
    methods::APPROVALS_LIST,
    methods::APPROVALS_DECIDE,
    methods::RUNTIME_DIGEST,
];

async fn call(target: &CoreTarget, method: &str, params: Value) -> Result<Value, String> {
    let (mut c, _) = target.connect_or_start().await?;
    c.call_raw(method, params).await.map_err(|e| e.to_string())
}

/// Answers one request line (`None`: not a request).
pub async fn answer(target: &CoreTarget, line: &str) -> Option<Value> {
    let req: Value = serde_json::from_str(line.trim()).ok()?;
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req["method"].as_str().unwrap_or_default();
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    if !ALLOWED.contains(&method) {
        return Some(json!({
            "id": id,
            "error": format!("method '{method}' is not available to the VS Code extension")
        }));
    }
    Some(match call(target, method, params).await {
        Ok(v) => json!({ "id": id, "result": v }),
        Err(e) => json!({ "id": id, "error": e }),
    })
}

/// `tumr rpc --stdio`: serves until stdin closes.
pub async fn stdio(target: &CoreTarget) -> ExitCode {
    let target = Arc::new(target.clone());
    let out = Arc::new(Mutex::new(tokio::io::stdout()));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut tasks = tokio::task::JoinSet::new();
    while let Ok(Some(line)) = lines.next_line().await {
        let (target, out) = (target.clone(), out.clone());
        tasks.spawn(async move {
            if let Some(resp) = answer(&target, &line).await {
                let mut o = out.lock().await;
                let _ = o.write_all(format!("{resp}\n").as_bytes()).await;
                let _ = o.flush().await;
            }
        });
        // forget the finished ones
        while tasks.try_join_next().is_some() {}
    }
    while tasks.join_next().await.is_some() {}
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn administration_stays_out_of_reach() {
        for m in [
            methods::INSTALL_START,
            methods::PROJECT_TRUST,
            methods::AUTH_CLIENT_CREATE,
            methods::SHUTDOWN,
            methods::ADMIN_RUN,
            methods::PROJECT_RUN,
        ] {
            assert!(!ALLOWED.contains(&m), "{m}");
        }
    }

    #[tokio::test]
    async fn refused_and_malformed_requests() {
        let target = CoreTarget {
            paths: tumr_core::paths::CorePaths::with_root(
                std::env::temp_dir().join("tumr-rpc-test"),
            ),
            pipe: r"\\.\pipe\tumr-rpc-test-no-core".into(),
        };
        let r = answer(&target, r#"{"id":7,"method":"install.start"}"#)
            .await
            .unwrap();
        assert_eq!(r["id"], 7);
        assert!(r["error"].as_str().unwrap().contains("not available"));
        assert!(answer(&target, "not json").await.is_none());
    }
}
