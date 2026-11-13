//! Device interaction API (DEV-005/006, step 2.5): app install / launch / stop / uninstall,
//! screenshot, UI tree, input, fold posture.
//!
//! Target device: the `target` serial if given, else the current session's device, else the
//! only emulator the product started, else the only online device. Emulators the product
//! started get gRPC (screenshot, touch, text, posture); other devices fall back to adb.

use std::path::{Path, PathBuf};

use umr_api::methods::{
    AppParams, InputAction, InputParams, InstallAppParams, PostureParams, TargetParams,
};
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_device::grpc::Emulator;

use crate::api_server::{ok, ApiState};
use crate::device_api;

/// The device a request acts on.
pub(crate) struct Target {
    pub serial: String,
    /// gRPC endpoint when the product started this emulator.
    pub grpc: Option<Emulator>,
    /// AVD name when the product started this emulator.
    pub avd: Option<String>,
}

fn unavailable(msg: impl Into<String>) -> RpcError {
    RpcError::new(codes::UNAVAILABLE, msg)
}

fn failed(msg: impl Into<String>) -> RpcError {
    RpcError::new(codes::INTERNAL_ERROR, msg)
}

pub(crate) fn target(state: &ApiState, wanted: Option<&str>) -> Result<Target, RpcError> {
    device_api::ensure_tracker(state)?;
    let emus = device_api::running(state);
    let make = |serial: &str| {
        let r = emus.iter().find(|r| r.serial == serial);
        Target {
            serial: serial.to_string(),
            grpc: r.and_then(|r| r.grpc_port.map(|p| Emulator::new(p, r.grpc_token.clone()))),
            avd: r.map(|r| r.avd.clone()),
        }
    };
    let online: Vec<String> = umr_device::adb::devices(state.adb_port)
        .map_err(|e| unavailable(e.to_string()))?
        .into_iter()
        .filter(umr_device::adb::Device::is_ready)
        .map(|d| d.serial)
        .collect();
    if let Some(w) = wanted {
        return if online.iter().any(|s| s == w) {
            Ok(make(w))
        } else {
            Err(unavailable(format!(
                "device {w} is not online (online: {online:?})"
            )))
        };
    }
    let session_serial = state
        .session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .filter(|s| s.state.has_device())
        .and_then(|s| s.serial.clone());
    if let Some(s) = session_serial.filter(|s| online.contains(s)) {
        return Ok(make(&s));
    }
    if let [only] = emus.as_slice() {
        return Ok(make(&only.serial));
    }
    match online.as_slice() {
        [only] => Ok(make(only)),
        [] => Err(unavailable(
            "no device is running; start one with `umr device use <profile>`",
        )),
        many => Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("several devices are online ({many:?}); pass `target`"),
        )),
    }
}

/// Width and height from a PNG header.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    Some((w, h))
}

/// Keeps the newest `keep` screenshots.
fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();
    let n = files.len().saturating_sub(keep);
    for f in files.into_iter().take(n) {
        let _ = std::fs::remove_file(f);
    }
}

pub fn screenshot(state: &ApiState, id: Id, p: &TargetParams) -> Response {
    let t = match target(state, p.target.as_deref()) {
        Ok(t) => t,
        Err(e) => return Response::err(Some(id), e),
    };
    let start = std::time::Instant::now();
    let scaled = |g: &Emulator, max: u32| {
        // a box `max` wide and far taller than any screen: the emulator fits the image into it
        // keeping the aspect ratio (a width alone is ignored)
        g.screenshot_png_at(max, max.saturating_mul(4))
    };
    let (png, source) = match &t.grpc {
        Some(g) => match p
            .max_width
            .map_or_else(|| g.screenshot_png(), |m| scaled(g, m))
        {
            Ok((png, _, _)) => (png, "grpc"),
            Err(e) => return Response::err(Some(id), failed(e)),
        },
        None => match umr_device::adb::shell_bytes(
            state.adb_port,
            &t.serial,
            "screencap -p",
            std::time::Duration::from_secs(30),
        ) {
            Ok(png) => (png, "adb"),
            Err(e) => return Response::err(Some(id), failed(e.to_string())),
        },
    };
    let Some((width, height)) = png_size(&png) else {
        return Response::err(Some(id), failed("the device returned no PNG image"));
    };
    let dir = state.data_root.join("screenshots");
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let path = dir.join(format!("{ms}-{}.png", t.serial));
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &png)) {
        return Response::err(Some(id), failed(format!("{}: {e}", path.display())));
    }
    prune(&dir, 50);
    let mut v = serde_json::json!({
        "serial": t.serial, "path": path, "width": width, "height": height,
        "bytes": png.len(), "source": source,
        "ms": u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    });
    // a scaled image: also the device's own size, in the image's orientation (taps use it)
    if p.max_width.is_some() {
        if let Some((dw, dh)) = device_size(state.adb_port, &t.serial) {
            let (dw, dh) = if (width > height) == (dw > dh) {
                (dw, dh)
            } else {
                (dh, dw)
            };
            v["device_width"] = serde_json::json!(dw);
            v["device_height"] = serde_json::json!(dh);
        }
    }
    ok(id, &v)
}

/// `wm size`: the override size if set, else the physical one.
fn device_size(port: u16, serial: &str) -> Option<(u32, u32)> {
    let out = umr_device::adb::shell(port, serial, "wm size").ok()?;
    let line = out
        .lines()
        .find(|l| l.starts_with("Override size:"))
        .or_else(|| out.lines().find(|l| l.starts_with("Physical size:")))?;
    let (w, h) = line.split_once(':')?.1.trim().split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

pub fn ui(state: &ApiState, id: Id, p: &TargetParams) -> Response {
    let t = match target(state, p.target.as_deref()) {
        Ok(t) => t,
        Err(e) => return Response::err(Some(id), e),
    };
    let start = std::time::Instant::now();
    match umr_device::ui::dump(state.adb_port, &t.serial) {
        Ok(nodes) => ok(
            id,
            &serde_json::json!({
                "serial": t.serial, "count": nodes.len(), "nodes": nodes,
                "ms": u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            }),
        ),
        Err(e) => Response::err(Some(id), failed(e)),
    }
}

fn need<T: Copy>(v: Option<T>, name: &str) -> Result<T, RpcError> {
    v.ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, format!("`{name}` is required")))
}

#[allow(clippy::many_single_char_names)] // t / p / s / g / x / y
fn do_input(state: &ApiState, t: &Target, p: &InputParams) -> Result<&'static str, RpcError> {
    let port = state.adb_port;
    let s = t.serial.as_str();
    match p.action {
        InputAction::Tap => {
            let (x, y) = (need(p.x, "x")?, need(p.y, "y")?);
            match &t.grpc {
                Some(g) => g.tap(x, y).map(|()| "grpc"),
                None => umr_device::ui::tap(port, s, x, y).map(|()| "adb"),
            }
        }
        InputAction::Swipe => {
            let from = (need(p.x, "x")?, need(p.y, "y")?);
            let to = (need(p.x2, "x2")?, need(p.y2, "y2")?);
            let ms = p.ms.unwrap_or(300).clamp(50, 5000);
            match &t.grpc {
                Some(g) => g.swipe(from, to, ms).map(|()| "grpc"),
                None => umr_device::ui::swipe(port, s, from, to, ms).map(|()| "adb"),
            }
        }
        InputAction::Text => {
            let text = p
                .text
                .as_deref()
                .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "`text` is required"))?;
            match &t.grpc {
                Some(g) => g.text(text).map(|()| "grpc"),
                None => umr_device::ui::text(port, s, text).map(|()| "adb"),
            }
        }
        InputAction::Key => {
            let key = p
                .key
                .as_deref()
                .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "`key` is required"))?;
            if umr_device::ui::keycode(key).is_none() {
                return Err(RpcError::new(
                    codes::INVALID_PARAMS,
                    format!("unknown key '{key}'"),
                ));
            }
            umr_device::ui::key(port, s, key).map(|()| "adb")
        }
    }
    .map_err(failed)
}

pub fn input(state: &ApiState, id: Id, p: &InputParams) -> Response {
    let r = target(state, p.target.as_deref()).and_then(|t| {
        let via = do_input(state, &t, p)?;
        Ok(serde_json::json!({ "serial": t.serial, "via": via }))
    });
    match r {
        Ok(v) => ok(id, &v),
        Err(e) => Response::err(Some(id), e),
    }
}

fn posture_name(p: umr_device::catalog::Posture) -> &'static str {
    use umr_device::catalog::Posture as P;
    match p {
        P::Closed => "closed",
        P::HalfOpened => "half-opened",
        P::Opened => "opened",
        P::Flipped => "flipped",
        P::Tent => "tent",
    }
}

/// `umr_<profile>_api<n>` → the catalogue profile.
fn profile_of_avd(avd: &str) -> Option<&'static umr_device::catalog::Profile> {
    let rest = avd.strip_prefix("umr_")?;
    let (id, _) = rest.rsplit_once("_api")?;
    umr_device::catalog::find(id)
}

pub fn posture(state: &ApiState, id: Id, p: &PostureParams) -> Response {
    let Some(want) = umr_device::catalog::Posture::parse(&p.posture) else {
        return Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                format!(
                    "unknown posture '{}'; use closed, half-opened, opened, flipped or tent",
                    p.posture
                ),
            ),
        );
    };
    let r = target(state, p.target.as_deref()).and_then(|t| {
        let (Some(g), Some(avd)) = (&t.grpc, &t.avd) else {
            return Err(unavailable(
                "posture control needs an emulator started by the product (gRPC)",
            ));
        };
        let fold = profile_of_avd(avd)
            .and_then(|pr| pr.fold.as_ref())
            .ok_or_else(|| {
                RpcError::new(
                    codes::INVALID_PARAMS,
                    format!("{avd} is not a foldable profile"),
                )
            })?;
        if !fold.postures.contains(&want) {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                format!("{avd} does not support posture '{}'", p.posture),
            ));
        }
        g.set_posture(want).map_err(failed)?;
        // what Android reports now (the device_state service follows the hinge sensor)
        std::thread::sleep(std::time::Duration::from_millis(700));
        let reported = umr_device::adb::shell(state.adb_port, &t.serial, "cmd device_state state")
            .ok()
            .and_then(|o| {
                o.lines()
                    .find(|l| l.contains("Committed state"))
                    .map(|l| l.trim().to_string())
            });
        Ok(serde_json::json!({
            "serial": t.serial, "posture": posture_name(want),
            "device_state": reported,
        }))
    });
    match r {
        Ok(v) => ok(id, &v),
        Err(e) => Response::err(Some(id), e),
    }
}

/// Installs an APK; returns its package name.
pub(crate) fn install_apk(state: &ApiState, serial: &str, apk: &str) -> Result<String, RpcError> {
    let path = PathBuf::from(apk);
    if !path.is_absolute()
        || path
            .extension()
            .is_none_or(|x| !x.eq_ignore_ascii_case("apk"))
    {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            "`apk` must be the absolute path of an .apk file",
        ));
    }
    if !path.is_file() {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("{apk} does not exist"),
        ));
    }
    let sdk = state.toolchains.join("android-sdk");
    let aapt2 = umr_device::app::find_aapt2(&sdk)
        .ok_or_else(|| unavailable("Android build-tools (aapt2) are not installed"))?;
    let package = umr_device::app::apk_package(&aapt2, &path).map_err(failed)?;
    let server = device_api::adb_server(state).map_err(unavailable)?;
    umr_device::app::install(&server, serial, &path).map_err(failed)?;
    tracing::info!(serial, package = %package, "app installed");
    Ok(package)
}

pub fn install(state: &ApiState, id: Id, p: &InstallAppParams) -> Response {
    let r = target(state, p.target.as_deref()).and_then(|t| {
        let package = install_apk(state, &t.serial, &p.apk)?;
        Ok(serde_json::json!({ "serial": t.serial, "package": package }))
    });
    match r {
        Ok(v) => ok(id, &v),
        Err(e) => Response::err(Some(id), e),
    }
}

fn check_package(p: &str) -> Result<(), RpcError> {
    if umr_device::app::valid_package(p) {
        Ok(())
    } else {
        Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("'{p}' is not a valid Android package name"),
        ))
    }
}

pub fn launch(state: &ApiState, id: Id, p: &AppParams) -> Response {
    let r = check_package(&p.package)
        .and_then(|()| target(state, p.target.as_deref()))
        .and_then(|t| {
            let l = umr_device::app::launch(state.adb_port, &t.serial, &p.package, p.activity.as_deref())
                .map_err(failed)?;
            tracing::info!(serial = %t.serial, component = %l.component, total_ms = ?l.total_ms, "app launched");
            crate::session_api::app_launched(
                state,
                &t.serial,
                &p.package,
                &format!("{} started in {} ms", l.component, l.total_ms.unwrap_or(0)),
            );
            Ok(serde_json::json!({
                "serial": t.serial, "component": l.component, "total_ms": l.total_ms, "pid": l.pid,
            }))
        });
    match r {
        Ok(v) => ok(id, &v),
        Err(e) => Response::err(Some(id), e),
    }
}

pub fn stop(state: &ApiState, id: Id, p: &AppParams) -> Response {
    let r = check_package(&p.package)
        .and_then(|()| target(state, p.target.as_deref()))
        .and_then(|t| {
            umr_device::app::force_stop(state.adb_port, &t.serial, &p.package).map_err(failed)?;
            crate::session_api::app_stopped(state, &t.serial, &p.package);
            Ok(serde_json::json!({ "serial": t.serial, "package": p.package }))
        });
    match r {
        Ok(v) => ok(id, &v),
        Err(e) => Response::err(Some(id), e),
    }
}

pub fn uninstall(state: &ApiState, id: Id, p: &AppParams) -> Response {
    let r = check_package(&p.package)
        .and_then(|()| target(state, p.target.as_deref()))
        .and_then(|t| {
            umr_device::app::uninstall(state.adb_port, &t.serial, &p.package).map_err(failed)?;
            Ok(serde_json::json!({ "serial": t.serial, "package": p.package }))
        });
    match r {
        Ok(v) => ok(id, &v),
        Err(e) => Response::err(Some(id), e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_header_and_profile_names() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&2400u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((1080, 2400)));
        assert_eq!(png_size(b"GIF89a"), None);
        assert_eq!(profile_of_avd("umr_fold_api36").map(|p| p.id), Some("fold"));
        assert!(profile_of_avd("umr_fold_api36").unwrap().fold.is_some());
        assert!(profile_of_avd("Pixel_8").is_none());
    }
}
