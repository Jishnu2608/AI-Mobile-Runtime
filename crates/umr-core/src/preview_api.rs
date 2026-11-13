//! Preview API (Phase 4): `preview.open`, `preview.status`, `preview.selftest`, `device.rotate`.
//!
//! The preview server (`umr-stream`) starts on first use and always shows the current session's
//! emulator; device switches and boots update it ([`refresh_target`]).

use umr_api::methods::{PreviewOpenParams, RotateParams, SelfTestParams};
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_stream::{PreviewServer, Target};

use crate::api_server::{ok, ApiState};
use crate::device_api;

fn fail(code: i64, msg: impl Into<String>) -> RpcError {
    RpcError::new(code, msg)
}

/// The preview server, started on first use (on the Core's Tokio runtime).
fn server(state: &ApiState) -> Result<&PreviewServer, RpcError> {
    if let Some(s) = state.preview.get() {
        return Ok(s);
    }
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| fail(codes::INTERNAL_ERROR, "no runtime for the preview server"))?;
    let s = handle
        .block_on(PreviewServer::start())
        .map_err(|e| fail(codes::INTERNAL_ERROR, format!("preview server: {e}")))?;
    tracing::info!(port = s.port(), "preview server listening on 127.0.0.1");
    let _ = state.preview.set(s);
    state
        .preview
        .get()
        .ok_or_else(|| fail(codes::INTERNAL_ERROR, "preview server"))
}

/// The emulator of the current session, as a preview target.
fn current_target(state: &ApiState) -> Option<Target> {
    let s = crate::session_api::current(state)?;
    if !s.state.has_device() {
        return None;
    }
    let r = device_api::running(state)
        .into_iter()
        .find(|r| r.avd == s.avd)?;
    Some(Target {
        serial: r.serial.clone(),
        grpc_port: r.grpc_port?,
        grpc_token: r.grpc_token.clone(),
        width: r.health.width,
        height: r.health.height,
    })
}

/// Points open preview windows at the session's current device (no-op before the first window).
pub(crate) fn refresh_target(state: &ApiState) {
    if let Some(s) = state.preview.get() {
        s.set_target(current_target(state));
    }
}

pub fn open(state: &ApiState, id: Id, p: &PreviewOpenParams) -> Response {
    let srv = match server(state) {
        Ok(s) => s,
        Err(e) => return Response::err(Some(id), e),
    };
    let target = current_target(state);
    srv.set_target(target.clone());
    let token = srv.grant(p.interact.unwrap_or(true));
    let session = crate::session_api::current(state);
    ok(
        id,
        &serde_json::json!({
            "url": srv.url(&token),
            "device": target.as_ref().map(|t| serde_json::json!({ "serial": t.serial, "width": t.width, "height": t.height })),
            "profile": session.as_ref().map(|s| s.profile.clone()),
            "state": session.as_ref().map(|s| s.state),
        }),
    )
}

pub fn status(state: &ApiState, id: Id) -> Response {
    refresh_target(state);
    let v = match state.preview.get() {
        Some(s) => serde_json::json!({
            "running": true, "port": s.port(), "windows": s.clients(),
            "device": s.target().map(|t| t.serial), "stats": s.stats(),
        }),
        None => serde_json::json!({ "running": false, "windows": 0 }),
    };
    ok(id, &v)
}

/// NFR targets (`master_build` §NFR): 02a pipeline p95 ≤ 20 ms, 02b end-to-end p50 ≤ 80 / p95 ≤ 150 ms,
/// 03 ≥ 30 fps.
fn verdict(
    st: &umr_stream::selftest::SelfTest,
    pipe: &umr_stream::stats::Report,
) -> serde_json::Value {
    // end to end in the window = touch → frame at the Core + hop to the window + draw
    let extra = pipe.ui.hop_p95 + pipe.ui.draw_p95;
    let e2e_p50 = st.latency.p50_ms + pipe.ui.draw_p50;
    let e2e_p95 = st.latency.p95_ms + extra;
    let window = pipe.ui.frames > 0;
    serde_json::json!({
        "nfr_02a": { "pipeline_p95_ms": pipe.pipeline_p95_ms, "target_ms": 20, "met": pipe.pipeline_p95_ms <= 20.0, "window_measured": window },
        "nfr_02b": { "p50_ms": (e2e_p50 * 10.0).round() / 10.0, "p95_ms": (e2e_p95 * 10.0).round() / 10.0, "target": "p50 <= 80, p95 <= 150", "met": st.detected > 0 && e2e_p50 <= 80.0 && e2e_p95 <= 150.0 },
        "nfr_03": { "fps": st.fps, "window_fps": pipe.ui.fps, "target": 30, "met": st.fps >= 30.0 },
        "host_overloaded": st.latency.p95_ms > 150.0 || st.fps < 30.0,
    })
}

pub fn selftest(state: &ApiState, id: Id, p: &SelfTestParams) -> Response {
    let Some(target) = current_target(state) else {
        return Response::err(
            Some(id),
            fail(
                codes::UNAVAILABLE,
                "no device is running; start one with `umr device use <profile>`",
            ),
        );
    };
    let taps = p.taps.unwrap_or(20).clamp(3, 100);
    let (port, serial) = (state.adb_port, target.serial.clone());
    let sh = |cmd: &str| umr_device::adb::shell(port, &serial, cmd);
    // known screen + touch feedback overlay (restored afterwards)
    let _ = sh("input keyevent KEYCODE_WAKEUP");
    let _ = sh("input keyevent KEYCODE_HOME");
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return Response::err(Some(id), fail(codes::INTERNAL_ERROR, "no runtime"));
    };
    let _ = sh("settings put system pointer_location 1");
    std::thread::sleep(std::time::Duration::from_millis(800));
    let r = handle.block_on(umr_stream::selftest::run(&target, taps));
    let _ = sh("settings put system pointer_location 0");
    // the drag may have moved the launcher to another page
    let _ = sh("input keyevent KEYCODE_HOME");
    match r {
        Ok(st) => {
            let pipe = state
                .preview
                .get()
                .map(PreviewServer::stats)
                .unwrap_or_default();
            tracing::info!(
                p50 = st.latency.p50_ms,
                p95 = st.latency.p95_ms,
                fps = st.fps,
                "preview self-test"
            );
            ok(
                id,
                &serde_json::json!({ "selftest": st, "pipeline": pipe, "verdict": verdict(&st, &pipe) }),
            )
        }
        Err(e) => Response::err(Some(id), fail(codes::INTERNAL_ERROR, e)),
    }
}

/// Rotates the current emulator 90° (console `rotate`; `right` = three left turns).
pub fn rotate(state: &ApiState, id: Id, p: &RotateParams) -> Response {
    let Some(s) = crate::session_api::current(state).filter(|s| s.state.has_device()) else {
        return Response::err(Some(id), fail(codes::UNAVAILABLE, "no device is running"));
    };
    let Some(r) = device_api::running(state)
        .into_iter()
        .find(|r| r.avd == s.avd)
    else {
        return Response::err(
            Some(id),
            fail(codes::UNAVAILABLE, "the session's emulator is not running"),
        );
    };
    let turns = match p.direction.as_deref().unwrap_or("left") {
        "left" => 1,
        "right" => 3,
        other => {
            return Response::err(
                Some(id),
                fail(
                    codes::INVALID_PARAMS,
                    format!("unknown direction '{other}'; use left or right"),
                ),
            )
        }
    };
    let res =
        umr_device::console::Console::connect(r.console_port, std::time::Duration::from_secs(3))
            .and_then(|mut c| (0..turns).try_for_each(|_| c.command("rotate").map(|_| ())));
    match res {
        Ok(()) => ok(
            id,
            &serde_json::json!({ "serial": r.serial, "rotated": p.direction.as_deref().unwrap_or("left") }),
        ),
        Err(e) => Response::err(Some(id), fail(codes::INTERNAL_ERROR, e)),
    }
}
