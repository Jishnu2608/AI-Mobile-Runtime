//! Desktop app (UI-001/002): a Tauri 2 window that talks to the Runtime Core and runs the
//! onboarding wizard (Scan → Project → Plan → Licences → Install → Ready).
//!
//! The app is a thin client, like the CLI: it starts the Core if needed, logs in with the
//! Core's admin token and forwards calls. Only the API methods the wizard needs can be called
//! from the web view ([`ALLOWED`]), so a bug in the page cannot reach anything else.
//! The wizard's position is saved in `<home>\wizard.json`, so it continues after a restart or
//! reboot.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::Value;
use tokio::sync::Mutex;
use umr_api::client::{Client, ClientError};
use umr_api::methods;
use umr_core::launcher::CoreTarget;

/// API methods the web view may call.
const ALLOWED: &[&str] = &[
    methods::INFO,
    methods::DOCTOR,
    methods::ENV_SCAN,
    methods::PROJECT_DETECT,
    methods::PROJECT_TRUST,
    methods::PROJECT_PLAN,
    methods::INSTALL_PREPARE,
    methods::INSTALL_ACCEPT_LICENCE,
    methods::INSTALL_START,
    methods::INSTALL_STATUS,
    methods::INSTALL_CANCEL,
    methods::COMPONENTS_LIST,
    methods::ADMIN_RUN,
];

struct Bridge {
    target: Result<CoreTarget, String>,
    client: Mutex<Option<Client>>,
}

impl Bridge {
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let target = self.target.as_ref().map_err(Clone::clone)?;
        let mut slot = self.client.lock().await;
        for attempt in 0..2 {
            if slot.is_none() {
                let (c, _) = target.connect_or_start().await?;
                *slot = Some(c);
            }
            let Some(client) = slot.as_mut() else {
                continue;
            };
            match client.call_raw(method, params.clone()).await {
                Ok(v) => return Ok(v),
                Err(ClientError::Rpc(e)) => return Err(e.message),
                Err(e) if attempt == 0 => {
                    // connection lost (Core restarted): reconnect once
                    let _ = e;
                    *slot = None;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("cannot reach the Runtime Core".into())
    }

    fn wizard_file(&self) -> Result<std::path::PathBuf, String> {
        let t = self.target.as_ref().map_err(Clone::clone)?;
        Ok(t.paths.root().join("wizard.json"))
    }
}

#[tauri::command]
async fn core_call(
    bridge: tauri::State<'_, Bridge>,
    method: String,
    params: Value,
) -> Result<Value, String> {
    if !ALLOWED.contains(&method.as_str()) {
        return Err(format!(
            "method '{method}' is not available to the desktop window"
        ));
    }
    bridge.call(&method, params).await
}

#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
#[tauri::command]
fn wizard_load(bridge: tauri::State<'_, Bridge>) -> Result<Value, String> {
    let f = bridge.wizard_file()?;
    match std::fs::read_to_string(&f) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| e.to_string()),
        Err(_) => Ok(Value::Null),
    }
}

#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
#[tauri::command]
fn wizard_save(bridge: tauri::State<'_, Bridge>, state: Value) -> Result<(), String> {
    let f = bridge.wizard_file()?;
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &f,
        serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn main() {
    let bridge = Bridge {
        target: CoreTarget::resolve(),
        client: Mutex::new(None),
    };
    tauri::Builder::default()
        .manage(bridge)
        .invoke_handler(tauri::generate_handler![
            core_call,
            wizard_load,
            wizard_save
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| eprintln!("umr-desktop: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_wizard_methods_are_allowed() {
        assert!(ALLOWED.contains(&methods::INSTALL_START));
        for m in [
            methods::SHUTDOWN,
            methods::AUTH_CLIENT_CREATE,
            methods::AUTH_CLIENT_LIST,
            methods::COMPONENTS_REMOVE,
            methods::COMPONENTS_REPAIR,
        ] {
            assert!(
                !ALLOWED.contains(&m),
                "{m} must not be reachable from the web view"
            );
        }
    }
}
