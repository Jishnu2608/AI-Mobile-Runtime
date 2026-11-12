//! Core side of INS-005/006: `components.list / remove / repair / clean` and `admin.run`.
//!
//! Changing components is refused while an install job runs. `admin.run` first checks whether
//! the setting is already on (then no UAC prompt is shown at all), and otherwise starts
//! `umr-elevate.exe` from the Core's own folder (never from PATH) through UAC.

use std::sync::atomic::Ordering;

use umr_api::methods::{AdminRunParams, AdminRunResult, RemoveComponentParams};
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_elevate::{describe_exit, exit, Action};
use umr_installer::layout::Layout;
use umr_installer::manage;
use umr_installer::InstallError;

use crate::api_server::{internal, ok, ApiState};

fn err(id: Id, code: i64, msg: impl Into<String>) -> Response {
    Response::err(Some(id), RpcError::new(code, msg))
}

fn install_running(state: &ApiState) -> bool {
    state
        .install_job
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|j| j.is_running())
}

fn manage_error(id: Id, e: &InstallError) -> Response {
    match e {
        InstallError::Unsupported(m) => err(id, codes::INVALID_PARAMS, m.clone()),
        other => internal(id, other),
    }
}

pub fn list(state: &ApiState, id: Id) -> Response {
    match manage::verify(&state.db) {
        Ok(components) => {
            let disk = manage::disk_usage(&Layout::new(&state.toolchains), &components);
            ok(
                id,
                &serde_json::json!({ "components": components, "disk": disk }),
            )
        }
        Err(e) => internal(id, &e),
    }
}

pub fn remove(state: &ApiState, id: Id, p: &RemoveComponentParams) -> Response {
    if install_running(state) {
        return err(
            id,
            codes::BUSY,
            "an install is running; try again when it has finished",
        );
    }
    match manage::remove(&state.db, &Layout::new(&state.toolchains), &p.id, p.force) {
        Ok(r) => {
            state.scan_stale.store(true, Ordering::Relaxed);
            tracing::info!(component = %r.id, freed = r.freed_bytes, "component removed");
            ok(id, &r)
        }
        Err(e) => manage_error(id, &e),
    }
}

pub fn repair(state: &ApiState, id: Id) -> Response {
    if install_running(state) {
        return err(
            id,
            codes::BUSY,
            "an install is running; try again when it has finished",
        );
    }
    match manage::repair(&state.db, &Layout::new(&state.toolchains)) {
        Ok(removed) => {
            state.scan_stale.store(true, Ordering::Relaxed);
            ok(id, &serde_json::json!({ "removed": removed }))
        }
        Err(e) => manage_error(id, &e),
    }
}

pub fn clean(state: &ApiState, id: Id) -> Response {
    if install_running(state) {
        return err(
            id,
            codes::BUSY,
            "an install is running; its downloads are in use",
        );
    }
    match manage::clean(&Layout::new(&state.toolchains)) {
        Ok(freed) => ok(id, &serde_json::json!({ "freed_bytes": freed })),
        Err(e) => internal(id, &e),
    }
}

/// Current state of the setting an action changes: Some(result) when nothing needs doing.
fn already(action: Action) -> Option<(&'static str, String)> {
    match action {
        Action::EnableWhpx => match umr_platform::system::whpx_status() {
            umr_platform::system::WhpxStatus::Usable => Some(("already-on", "Windows Hypervisor Platform is on and usable".into())),
            umr_platform::system::WhpxStatus::NoHypervisor => Some((
                "needs-firmware",
                "Windows Hypervisor Platform is installed but no hypervisor is running: restart Windows; if it stays off, turn on CPU virtualization (VT-x / AMD-V / SVM) in the BIOS/UEFI settings. Software cannot change that.".into(),
            )),
            _ => None,
        },
        Action::EnableLongPaths => umr_platform::os::long_paths_enabled()
            .ok()
            .filter(|on| *on)
            .map(|_| ("already-on", "Win32 long paths are already enabled".into())),
    }
}

pub fn admin_run(state: &ApiState, id: Id, client: Option<&str>, p: &AdminRunParams) -> Response {
    let Some(action) = Action::parse(&p.action) else {
        let names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        return err(
            id,
            codes::INVALID_PARAMS,
            format!(
                "unknown action '{}'; allowed: {}",
                p.action,
                names.join(", ")
            ),
        );
    };
    let audit = |status: &str| {
        let db = state
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = db.audit(
            umr_installer::now_unix(),
            client,
            "admin.run",
            Some(&format!("{} -> {status}", action.name())),
        );
    };
    let reply = |id: Id, status: &str, message: String| {
        ok(
            id,
            &AdminRunResult {
                action: action.name().into(),
                status: status.into(),
                message,
            },
        )
    };
    if let Some((status, message)) = already(action) {
        return reply(id, status, message);
    }
    let helper = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name("umr-elevate.exe"),
        Err(e) => return internal(id, &e),
    };
    if !helper.is_file() {
        return err(
            id,
            codes::INTERNAL_ERROR,
            format!("elevated helper not found at {}", helper.display()),
        );
    }
    tracing::info!(action = action.name(), client = ?client, "starting the elevated helper (UAC prompt)");
    audit("requested");
    let result = umr_platform::elevate::run_as_admin(&helper, &[action.name()]);
    state.scan_stale.store(true, Ordering::Relaxed);
    let (status, message) = match result {
        Ok(umr_platform::elevate::Elevation::Declined) => (
            "declined",
            "the Windows administrator prompt was declined; nothing was changed".to_string(),
        ),
        Ok(umr_platform::elevate::Elevation::Exited(code)) => {
            let status = match code {
                exit::DONE => "done",
                exit::RESTART_REQUIRED => "restart-required",
                _ => "failed",
            };
            (
                status,
                format!("{}: {}", action.title(), describe_exit(code)),
            )
        }
        Err(e) => ("failed", format!("cannot start the elevated helper: {e}")),
    };
    audit(status);
    tracing::info!(action = action.name(), status, "elevated helper finished");
    reply(id, status, message)
}
