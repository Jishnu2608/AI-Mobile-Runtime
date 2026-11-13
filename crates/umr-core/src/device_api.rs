//! Device API.
//!
//! * `device.list` (DEV-001/007): starts the product's private adb server and device tracking on
//!   first use, then returns the tracked devices, the emulators the product started and the other
//!   adb servers that were left alone.
//! * `device.profiles` / `device.create` (DEV-002/008): size-class catalogue, lazy AVD creation.
//! * `device.boot` / `device.stop` (DEV-003/009): headless boot with health check and warm
//!   snapshot; stop.

use std::time::Duration;

use umr_api::methods::{BootDeviceParams, CreateDeviceParams, DeviceRefParams, ProfilesParams};
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_device::server::AdbServer;
use umr_device::tracker::Tracker;

use crate::api_server::{ok, ApiState};

/// The user's own `ANDROID_ADB_SERVER_PORT`, if they set one (reported, never used by us).
fn user_adb_port() -> Option<u16> {
    std::env::var("ANDROID_ADB_SERVER_PORT")
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The product's adb server with the isolated environment (not started yet).
pub(crate) fn adb_server(state: &ApiState) -> Result<AdbServer, String> {
    let sdk = state.toolchains.join("android-sdk");
    let adb = sdk.join("platform-tools").join("adb.exe");
    if !adb.is_file() {
        return Err("Android platform-tools (adb) are not installed yet; run `umr install` for a project first".into());
    }
    let dirs = umr_env::recipe::IsolationDirs::under(&state.toolchains);
    dirs.create().map_err(|e| e.to_string())?;
    let ctx = umr_env::ScanContext::from_system(state.toolchains.clone());
    let sel = umr_env::recipe::Selection {
        android_sdk: Some(sdk),
        adb_port: Some(state.adb_port),
        ..Default::default()
    };
    let env = umr_env::recipe::child_env(&ctx, &sel);
    Ok(AdbServer::new(&adb, state.adb_port, env))
}

fn start_tracker(state: &ApiState) -> Result<Tracker, String> {
    Ok(Tracker::start(adb_server(state)?))
}

/// Starts the private adb server and tracking if they are not running yet.
pub(crate) fn ensure_tracker(state: &ApiState) -> Result<(), RpcError> {
    let mut slot = state
        .adb
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_none() {
        let t = start_tracker(state).map_err(|e| RpcError::new(codes::UNAVAILABLE, e))?;
        tracing::info!(port = state.adb_port, "adb tracking started");
        *slot = Some(t);
    }
    Ok(())
}

/// Reports of the running emulators the product started (exited ones are dropped).
pub(crate) fn running(state: &ApiState) -> Vec<umr_device::emulator::BootReport> {
    let mut map = state
        .emulators
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    map.retain(|name, inst| {
        let alive = inst.is_running();
        if !alive {
            tracing::warn!(avd = %name, "emulator exited");
        }
        alive
    });
    map.values().map(|i| i.report.clone()).collect()
}

pub fn list(state: &ApiState, id: Id) -> Response {
    let emulators = running(state);
    if let Err(e) = ensure_tracker(state) {
        return Response::err(Some(id), e);
    }
    let snapshot = {
        let slot = state
            .adb
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.as_ref()
            .map(|t| t.wait_ready(Duration::from_secs(10)))
            .unwrap_or_default()
    };
    let coexistence = umr_device::foreign::scan(state.adb_port, user_adb_port());
    ok(
        id,
        &serde_json::json!({
            "server": {
                "port": snapshot.port,
                "protocol": snapshot.protocol,
                "connected": snapshot.connected,
                "last_error": snapshot.last_error,
                "changes": snapshot.changes,
            },
            "devices": snapshot.devices,
            "emulators": emulators,
            "coexistence": coexistence,
        }),
    )
}

const TAG: &str = "google_apis";
const ABI: &str = "x86_64";

/// API level to use: the requested one, else the newest installed `google_apis` image.
pub(crate) fn api_level(
    state: &ApiState,
    wanted: Option<&str>,
) -> Result<(std::path::PathBuf, String), RpcError> {
    let sdk = state.toolchains.join("android-sdk");
    let installed = umr_device::avd::installed_apis(&sdk, TAG, ABI);
    match wanted {
        Some(a) if installed.iter().any(|i| i == a) => Ok((sdk, a.to_string())),
        Some(a) => Err(RpcError::new(
            codes::UNAVAILABLE,
            format!(
                "no {TAG} {ABI} system image for API {a} is installed (installed: {installed:?})"
            ),
        )),
        None => installed.first().cloned().map(|a| (sdk, a)).ok_or_else(|| {
            RpcError::new(
                codes::UNAVAILABLE,
                "no emulator system image is installed yet; run `umr install` for a project first",
            )
        }),
    }
}

fn avd_home(state: &ApiState) -> std::path::PathBuf {
    umr_env::recipe::IsolationDirs::under(&state.toolchains).avd_home
}

pub fn profiles(state: &ApiState, id: Id, p: &ProfilesParams) -> Response {
    let api = match api_level(state, p.api.as_deref()) {
        Ok((_, a)) => Some(a),
        Err(_) if p.api.is_none() => None, // catalogue is still useful without an image
        Err(e) => return Response::err(Some(id), e),
    };
    let home = avd_home(state);
    let list: Vec<serde_json::Value> = umr_device::catalog::PROFILES
        .iter()
        .map(|prof| {
            let avd = api
                .as_deref()
                .map(|a| umr_device::avd::info(&home, prof.id, a));
            serde_json::json!({ "profile": prof, "avd": avd })
        })
        .collect();
    ok(id, &serde_json::json!({ "api": api, "profiles": list }))
}

pub fn create(state: &ApiState, id: Id, p: &CreateDeviceParams) -> Response {
    let Some(prof) = umr_device::catalog::find(&p.profile) else {
        let ids: Vec<&str> = umr_device::catalog::PROFILES.iter().map(|x| x.id).collect();
        return Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                format!(
                    "unknown profile '{}'; choose one of {}",
                    p.profile,
                    ids.join(", ")
                ),
            ),
        );
    };
    let (sdk, api) = match api_level(state, p.api.as_deref()) {
        Ok(v) => v,
        Err(e) => return Response::err(Some(id), e),
    };
    match umr_device::avd::ensure(&avd_home(state), &sdk, prof, &api, TAG, ABI) {
        Ok(info) => {
            tracing::info!(avd = %info.name, profile = prof.id, api = %api, "AVD ready");
            ok(id, &info)
        }
        Err(e) => Response::err(Some(id), RpcError::new(codes::INTERNAL_ERROR, e)),
    }
}

/// Resolves profile + API to the AVD (created lazily).
pub(crate) fn resolve_avd(
    state: &ApiState,
    profile: &str,
    api: Option<&str>,
) -> Result<umr_device::avd::AvdInfo, RpcError> {
    let Some(prof) = umr_device::catalog::find(profile) else {
        let ids: Vec<&str> = umr_device::catalog::PROFILES.iter().map(|x| x.id).collect();
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!(
                "unknown profile '{profile}'; choose one of {}",
                ids.join(", ")
            ),
        ));
    };
    let (sdk, api) = api_level(state, api)?;
    umr_device::avd::ensure(&avd_home(state), &sdk, prof, &api, TAG, ABI)
        .map_err(|e| RpcError::new(codes::INTERNAL_ERROR, e))
}

/// Removes the AVD from the "booting" set when the boot ends (also on panic).
struct BootGuard<'a> {
    state: &'a ApiState,
    name: String,
}

impl Drop for BootGuard<'_> {
    fn drop(&mut self) {
        self.state
            .booting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.name);
    }
}

pub fn boot(state: &ApiState, id: Id, p: &BootDeviceParams) -> Response {
    let avd = match resolve_avd(state, &p.profile, p.api.as_deref()) {
        Ok(a) => a,
        Err(e) => return Response::err(Some(id), e),
    };
    match boot_avd(state, &avd, p.cold, p.window, p.gpu.as_deref()) {
        Ok(r) => ok(id, &r),
        Err(e) => Response::err(Some(id), e),
    }
}

/// Boots an AVD (or returns the running report); shared by `device.boot` and `session.start`.
pub(crate) fn boot_avd(
    state: &ApiState,
    avd: &umr_device::avd::AvdInfo,
    cold: bool,
    window: bool,
    gpu: Option<&str>,
) -> Result<umr_device::emulator::BootReport, RpcError> {
    let gpu = gpu.unwrap_or("host").to_string();
    if !umr_device::emulator::GPU_MODES.contains(&gpu.as_str()) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!(
                "unknown GPU mode '{gpu}'; choose one of {}",
                umr_device::emulator::GPU_MODES.join(", ")
            ),
        ));
    }
    if let Some(r) = running(state).into_iter().find(|r| r.avd == avd.name) {
        return Ok(r);
    }
    if !state
        .booting
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(avd.name.clone())
    {
        return Err(RpcError::new(
            codes::BUSY,
            format!("{} is already booting", avd.name),
        ));
    }
    let _guard = BootGuard {
        state,
        name: avd.name.clone(),
    };
    ensure_tracker(state)?;
    let sdk = state.toolchains.join("android-sdk");
    let emulator = sdk.join("emulator").join("emulator.exe");
    if !emulator.is_file() {
        return Err(RpcError::new(
            codes::UNAVAILABLE,
            "the Android emulator is not installed yet; run `umr install` for a project first",
        ));
    }
    umr_env::recipe::IsolationDirs::under(&state.toolchains)
        .create()
        .map_err(|e| RpcError::new(codes::INTERNAL_ERROR, e.to_string()))?;
    let ctx = umr_env::ScanContext::from_system(state.toolchains.clone());
    let sel = umr_env::recipe::Selection {
        android_sdk: Some(sdk),
        adb_port: Some(state.adb_port),
        ..Default::default()
    };
    let mut opts = umr_device::emulator::BootOptions::new(
        &emulator,
        &avd.name,
        std::path::Path::new(&avd.dir),
        state.adb_port,
    );
    opts.supervisor = state.supervisor.clone();
    opts.ports = state.ports.clone();
    opts.env = umr_env::recipe::child_env(&ctx, &sel);
    opts.log_dir = state.data_root.join("logs");
    opts.discovery_dirs = umr_env::recipe::emulator_discovery_dirs(&ctx);
    opts.force_cold = cold;
    opts.window = window;
    opts.gpu = gpu;
    tracing::info!(avd = %avd.name, cold, "booting emulator");
    match umr_device::emulator::boot(&opts) {
        Ok(inst) => {
            let r = inst.report.clone();
            tracing::info!(
                avd = %r.avd, serial = %r.serial, mode = ?r.mode, seconds = r.seconds,
                boot_completed = r.boot_completed_seconds, attempts = r.attempts,
                snapshot_saved = r.snapshot_saved, cold_reason = ?r.cold_reason,
                lit = r.health.lit, "emulator ready"
            );
            state
                .emulators
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(r.avd.clone(), inst);
            Ok(r)
        }
        Err(e) => {
            tracing::error!(avd = %avd.name, error = %e, "emulator boot failed");
            Err(RpcError::new(codes::INTERNAL_ERROR, e))
        }
    }
}

/// Stops the emulator of an AVD the product started; returns whether it was running.
pub(crate) fn stop_avd(state: &ApiState, name: &str) -> bool {
    let inst = state
        .emulators
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(name);
    let stopped = inst.is_some();
    if let Some(i) = inst {
        i.stop();
        tracing::info!(avd = %name, "emulator stopped");
    }
    stopped
}

pub fn stop(state: &ApiState, id: Id, p: &DeviceRefParams) -> Response {
    let name = match api_level(state, p.api.as_deref()) {
        Ok((_, api)) => umr_device::avd::avd_name(&p.profile, &api),
        Err(e) => return Response::err(Some(id), e),
    };
    let stopped = stop_avd(state, &name);
    ok(id, &serde_json::json!({ "avd": name, "stopped": stopped }))
}

/// Takes the AVD's emulator out of the running set and stops it on a background thread (a device
/// switch boots the next one meanwhile).
pub(crate) fn stop_avd_background(state: &ApiState, name: &str) {
    let inst = state
        .emulators
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(name);
    if let Some(i) = inst {
        let name = name.to_string();
        std::thread::spawn(move || {
            i.stop();
            tracing::info!(avd = %name, "emulator stopped");
        });
    }
}

/// Stops every emulator the product started (Core shutdown).
pub fn stop_all(state: &ApiState) {
    let all: Vec<_> = state
        .emulators
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain()
        .collect();
    for (name, inst) in all {
        inst.stop();
        tracing::info!(avd = %name, "emulator stopped");
    }
}
