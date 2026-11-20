//! Device API.
//!
//! * `device.list` (DEV-001/007): starts the product's private adb server and device tracking on
//!   first use, then returns the tracked devices, the emulators the product started and the other
//!   adb servers that were left alone.
//! * `device.profiles` / `device.create` (DEV-002/008): size-class catalogue, lazy AVD creation.
//! * `device.boot` / `device.stop` (DEV-003/009): headless boot with health check and warm
//!   snapshot; stop.

use std::path::Path;
use std::time::Duration;

use tumr_api::methods::{BootDeviceParams, CreateDeviceParams, DeviceRefParams, ProfilesParams};
use tumr_api::rpc::{codes, Id, Response, RpcError};
use tumr_device::server::AdbServer;
use tumr_device::tracker::Tracker;

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
        return Err("Android platform-tools (adb) are not installed yet; run `tumr install` for a project first".into());
    }
    let dirs = tumr_env::recipe::IsolationDirs::under(&state.toolchains);
    dirs.create().map_err(|e| e.to_string())?;
    let ctx = tumr_env::ScanContext::from_system(state.toolchains.clone());
    let sel = tumr_env::recipe::Selection {
        android_sdk: Some(sdk),
        adb_port: Some(state.adb_port),
        ..Default::default()
    };
    let env = tumr_env::recipe::child_env(&ctx, &sel);
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
pub(crate) fn running(state: &ApiState) -> Vec<tumr_device::emulator::BootReport> {
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
    let coexistence = tumr_device::foreign::scan(state.adb_port, user_adb_port());
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
    let installed = tumr_device::avd::installed_apis(&sdk, TAG, ABI);
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
                "no emulator system image is installed yet; run `tumr install` for a project first",
            )
        }),
    }
}

fn avd_home(state: &ApiState) -> std::path::PathBuf {
    tumr_env::recipe::IsolationDirs::under(&state.toolchains).avd_home
}

pub fn profiles(state: &ApiState, id: Id, p: &ProfilesParams) -> Response {
    let api = match api_level(state, p.api.as_deref()) {
        Ok((_, a)) => Some(a),
        Err(_) if p.api.is_none() => None, // catalogue is still useful without an image
        Err(e) => return Response::err(Some(id), e),
    };
    let home = avd_home(state);
    let list: Vec<serde_json::Value> = tumr_device::catalog::PROFILES
        .iter()
        .map(|prof| {
            let avd = api
                .as_deref()
                .map(|a| tumr_device::avd::info(&home, prof.id, a));
            // AUD-P1-45: switching to it starts from a saved snapshot (seconds), not a cold boot
            let warm = api.as_deref().and_then(|a| {
                let host = tumr_device::catalog::host(prof.id)?;
                let info = tumr_device::avd::info(&home, host.id, a);
                Some(snapshot_ready(Path::new(&info.dir)))
            });
            let preparing = api.as_deref().is_some_and(|a| {
                tumr_device::catalog::host(prof.id).is_some_and(|h| {
                    state
                        .booting
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .contains(&tumr_device::avd::info(&home, h.id, a).name)
                })
            });
            serde_json::json!({ "profile": prof, "avd": avd, "warm": warm, "preparing": preparing })
        })
        .collect();
    ok(id, &serde_json::json!({ "api": api, "profiles": list }))
}

/// Whether the AVD in `dir` has the product's warm snapshot (and its record).
fn snapshot_ready(dir: &Path) -> bool {
    tumr_device::emulator::read_record(dir).is_some()
        && dir
            .join("snapshots")
            .join(tumr_device::emulator::SNAPSHOT)
            .is_dir()
}

/// `device.prepare` (AUD-P1-45): boots a profile's emulator once in the background so its warm
/// snapshot exists before the user first switches to it, then stops it again.
pub fn prepare(state: &ApiState, id: Id, p: &CreateDeviceParams) -> Response {
    let Some(prof) =
        tumr_device::catalog::find(&p.profile).and_then(|x| tumr_device::catalog::host(x.id))
    else {
        return Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                format!("unknown profile '{}'", p.profile),
            ),
        );
    };
    let (sdk, api) = match api_level(state, p.api.as_deref()) {
        Ok(v) => v,
        Err(e) => return Response::err(Some(id), e),
    };
    let info = match tumr_device::avd::ensure(&avd_home(state), &sdk, prof, &api, TAG, ABI) {
        Ok(i) => i,
        Err(e) => return Response::err(Some(id), RpcError::new(codes::INTERNAL_ERROR, e)),
    };
    let answer = |status: &str| {
        ok(
            id.clone(),
            &serde_json::json!({ "profile": p.profile, "status": status }),
        )
    };
    if running(state).iter().any(|r| r.avd == info.name) {
        return answer("running");
    }
    if snapshot_ready(Path::new(&info.dir)) {
        return answer("ready");
    }
    let Some(weak) = state.self_ref.get().cloned() else {
        return Response::err(
            Some(id),
            RpcError::new(codes::INTERNAL_ERROR, "no Core handle"),
        );
    };
    let name = info.name.clone();
    let spawned = std::thread::Builder::new()
        .name("tumr-prepare".into())
        .spawn(move || {
            let Some(state) = weak.upgrade() else { return };
            match boot_avd(&state, &info, false, false, None) {
                Ok(b) => {
                    tracing::info!(avd = %name, seconds = b.seconds, saved = b.snapshot_saved, "prepared");
                    // stop it unless the user switched to it meanwhile
                    let in_use = crate::session_api::current(&state)
                        .is_some_and(|s| s.avd == name);
                    if !in_use {
                        stop_avd_background(&state, &name);
                    }
                }
                Err(e) => tracing::warn!(avd = %name, error = %e.message, "preparing failed"),
            }
        });
    match spawned {
        Ok(_) => answer("preparing"),
        Err(e) => Response::err(
            Some(id),
            RpcError::new(codes::INTERNAL_ERROR, e.to_string()),
        ),
    }
}

pub fn create(state: &ApiState, id: Id, p: &CreateDeviceParams) -> Response {
    let Some(prof) = tumr_device::catalog::find(&p.profile) else {
        let ids: Vec<&str> = tumr_device::catalog::PROFILES
            .iter()
            .map(|x| x.id)
            .collect();
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
    match tumr_device::avd::ensure(&avd_home(state), &sdk, prof, &api, TAG, ABI) {
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
) -> Result<tumr_device::avd::AvdInfo, RpcError> {
    // SNAP-003: compact and large are shown on the standard phone's emulator
    let Some(prof) = tumr_device::catalog::host(profile) else {
        let ids: Vec<&str> = tumr_device::catalog::PROFILES
            .iter()
            .map(|x| x.id)
            .collect();
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!(
                "unknown profile '{profile}'; choose one of {}",
                ids.join(", ")
            ),
        ));
    };
    let (sdk, api) = api_level(state, api)?;
    tumr_device::avd::ensure(&avd_home(state), &sdk, prof, &api, TAG, ABI)
        .map_err(|e| RpcError::new(codes::INTERNAL_ERROR, e))
}

/// SNAP-003: sets the screen size and density for `profile` on its (shared) emulator: another
/// size for compact and large, the emulator's own for standard. Profiles with their own emulator
/// are left alone. `adb shell wm` takes about a second; the running app gets a configuration
/// change and stays. Returns the command applied, or the error.
pub(crate) fn apply_display(
    state: &ApiState,
    serial: &str,
    profile: &str,
) -> Option<Result<String, String>> {
    let cmd = tumr_device::catalog::display_command(profile)?;
    Some(
        tumr_device::adb::shell(state.adb_port, serial, &cmd)
            .map(|_| cmd)
            .map_err(|e| e.to_string()),
    )
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
        Ok(r) => {
            let _ = apply_display(state, &r.serial, &p.profile);
            ok(id, &r)
        }
        Err(e) => Response::err(Some(id), e),
    }
}

/// Boots an AVD (or returns the running report); shared by `device.boot` and `session.start`.
pub(crate) fn boot_avd(
    state: &ApiState,
    avd: &tumr_device::avd::AvdInfo,
    cold: bool,
    window: bool,
    gpu: Option<&str>,
) -> Result<tumr_device::emulator::BootReport, RpcError> {
    let gpu = gpu.unwrap_or("host").to_string();
    if !tumr_device::emulator::GPU_MODES.contains(&gpu.as_str()) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!(
                "unknown GPU mode '{gpu}'; choose one of {}",
                tumr_device::emulator::GPU_MODES.join(", ")
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
            "the Android emulator is not installed yet; run `tumr install` for a project first",
        ));
    }
    tumr_env::recipe::IsolationDirs::under(&state.toolchains)
        .create()
        .map_err(|e| RpcError::new(codes::INTERNAL_ERROR, e.to_string()))?;
    let ctx = tumr_env::ScanContext::from_system(state.toolchains.clone());
    let sel = tumr_env::recipe::Selection {
        android_sdk: Some(sdk),
        adb_port: Some(state.adb_port),
        ..Default::default()
    };
    let mut opts = tumr_device::emulator::BootOptions::new(
        &emulator,
        &avd.name,
        std::path::Path::new(&avd.dir),
        state.adb_port,
    );
    opts.supervisor = state.supervisor.clone();
    opts.ports = state.ports.clone();
    opts.env = tumr_env::recipe::child_env(&ctx, &sel);
    opts.log_dir = state.data_root.join("logs");
    opts.discovery_dirs = tumr_env::recipe::emulator_discovery_dirs(&ctx);
    opts.force_cold = cold;
    opts.window = window;
    opts.gpu = gpu;
    tracing::info!(avd = %avd.name, cold, "booting emulator");
    // DEV-003b: the console token lands in the user profile whatever the environment says
    let token = tumr_device::console::auth_token_file();
    let token_before = token.as_ref().is_some_and(|t| t.is_file());
    match tumr_device::emulator::boot(&opts) {
        Ok(inst) => {
            if let Some(t) = &token {
                if tumr_device::console::note_token_created(&state.data_root, t, token_before) {
                    tracing::info!(token = %t.display(), "emulator created its console token file");
                }
            }
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
        Ok((_, api)) => tumr_device::avd::avd_name(&p.profile, &api),
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
