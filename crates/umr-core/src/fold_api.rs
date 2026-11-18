//! AUD-P1-40…46: foldables that behave like the real thing.
//!
//! The hinge is driven by its **angle** (the emulator's hinge sensor, gRPC `setPhysicalModel`),
//! as when a person opens the phone that far; Android's posture (`device_state`: closed /
//! half-opened / opened) follows from the AVD's angle ranges, and Jetpack `WindowManager` reports
//! the matching `FoldingFeature`. Closed, a foldable with a cover screen switches to it:
//! Android's display is set to the cover size (`wm size`, so the app lays itself out for that
//! screen) and drawn centred; the emulator shows the same centred rectangle
//! (`hw.displayRegion.0.1`), and the preview stream does too, with touches offset into it.
//! Opened again, the inner screen comes back (`wm size reset`).

use std::sync::PoisonError;
use std::time::{Duration, Instant};

use umr_api::methods::PostureParams;
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_device::catalog::{Fold, Posture, Profile};

use crate::api_server::{ok, ApiState};

/// The hinge of the running foldable as the Core last set it.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldState {
    pub serial: String,
    pub avd: String,
    /// Degrees: 0 = closed flat, 180 = open flat.
    pub angle: f32,
    pub posture: Posture,
    /// The cover screen in use (x, y, width, height inside the inner screen), when closed.
    pub cover: Option<(u32, u32, u32, u32)>,
}

/// Angle steps offered in the preview and the CLI.
pub const STEPS: [f32; 6] = [0.0, 30.0, 60.0, 90.0, 120.0, 180.0];

/// The posture for `angle` from the AVD's ranges ("0-30, 30-150, 150-180") the way Android reads
/// them: a range includes its **end** (measured on the API 36 emulator: 30° is closed, 31° half
/// open, 150° half open, 151° open).
#[must_use]
pub fn posture_at(fold: &Fold, angle: f32) -> Posture {
    let ranges: Vec<(f32, f32)> = fold
        .posture_angles
        .split(',')
        .filter_map(|r| {
            let (a, b) = r.trim().split_once('-')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .collect();
    for (i, (_, b)) in ranges.iter().enumerate() {
        if angle <= *b {
            if let Some(p) = fold.postures.get(i) {
                return *p;
            }
        }
    }
    Posture::Opened
}

/// The angle a posture name stands for (closed 0°, half open 90°, open 180°).
#[must_use]
pub fn angle_of(p: Posture) -> f32 {
    match p {
        Posture::Closed => 0.0,
        Posture::HalfOpened => 90.0,
        _ => 180.0,
    }
}

/// Android's name for a posture in `cmd device_state state`.
fn state_name(p: Posture) -> &'static str {
    match p {
        Posture::Closed => "CLOSED",
        Posture::HalfOpened => "HALF_OPENED",
        Posture::Opened => "OPENED",
        Posture::Flipped => "FLIPPED",
        Posture::Tent => "TENT",
    }
}

#[must_use]
pub fn posture_name(p: Posture) -> &'static str {
    match p {
        Posture::Closed => "closed",
        Posture::HalfOpened => "half-opened",
        Posture::Opened => "opened",
        Posture::Flipped => "flipped",
        Posture::Tent => "tent",
    }
}

/// "Closed", "Half open 90°", "Open (flat)": the posture in words.
#[must_use]
pub fn label(p: Posture, angle: f32) -> String {
    match p {
        Posture::Closed => "Closed".into(),
        Posture::Opened if angle >= 179.5 => "Open (flat)".into(),
        Posture::Opened => format!("Open {angle:.0}°"),
        Posture::HalfOpened => format!("Half open {angle:.0}°"),
        Posture::Flipped => "Flipped".into(),
        Posture::Tent => "Tent".into(),
    }
}

/// `umr_<profile>_api<n>` → the catalogue profile.
#[must_use]
pub fn profile_of_avd(avd: &str) -> Option<&'static Profile> {
    let rest = avd.strip_prefix("umr_")?;
    let (id, _) = rest.rsplit_once("_api")?;
    umr_device::catalog::find(id)
}

/// The fold state of the emulator `serial` runs (`None`: not a foldable, or nothing set yet).
#[must_use]
pub fn current(state: &ApiState, serial: &str, avd: &str) -> Option<FoldState> {
    state
        .fold
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .filter(|f| f.serial == serial && f.avd == avd)
}

/// The cover rectangle the preview shows for this emulator (when closed).
#[must_use]
pub fn view(state: &ApiState, serial: &str, avd: &str) -> Option<(u32, u32, u32, u32)> {
    current(state, serial, avd).and_then(|f| f.cover)
}

/// The JSON a client sees for a fold state.
fn describe(f: &FoldState, fold: &Fold) -> serde_json::Value {
    serde_json::json!({
        "serial": f.serial,
        "kind": fold.kind,
        "angle": f.angle,
        "posture": posture_name(f.posture),
        "label": label(f.posture, f.angle),
        "steps": STEPS,
        "cover": f.cover.map(|(x, y, w, h)| serde_json::json!({
            "x": x, "y": y, "width": w, "height": h, "label": fold.cover_label })),
    })
}

/// Android's committed device state (`CLOSED`, …), when it can be read.
fn device_state(port: u16, serial: &str) -> Option<String> {
    let out = umr_device::adb::shell(port, serial, "cmd device_state state").ok()?;
    out.lines()
        .find(|l| l.contains("Committed state"))
        .and_then(|l| l.split("name='").nth(1))
        .and_then(|r| r.split('\'').next())
        .map(str::to_string)
}

/// AUD-P2-46: waits until Android has committed `want` (instead of a fixed sleep).
fn wait_for_state(port: u16, serial: &str, want: &str, limit: Duration) -> Option<String> {
    let start = Instant::now();
    loop {
        let now = device_state(port, serial);
        if now.as_deref() == Some(want) || start.elapsed() >= limit {
            return now;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Sets the hinge of the running foldable to `angle` and switches between cover and inner
/// screen as needed.
///
/// # Errors
/// Not a foldable started by the product, or the emulator or adb refused.
pub fn set_angle(
    state: &ApiState,
    serial: &str,
    avd: &str,
    grpc: &umr_device::grpc::Emulator,
    angle: f32,
) -> Result<serde_json::Value, RpcError> {
    let start = Instant::now();
    let profile = profile_of_avd(avd).ok_or_else(|| {
        RpcError::new(
            codes::INVALID_PARAMS,
            format!("{avd} is not a catalogue device"),
        )
    })?;
    let fold = profile.fold.as_ref().ok_or_else(|| {
        RpcError::new(
            codes::INVALID_PARAMS,
            format!("{avd} is not a foldable profile"),
        )
    })?;
    let angle = angle.clamp(0.0, 180.0);
    let posture = posture_at(fold, angle);
    let before = current(state, serial, avd);
    grpc.set_hinge_angle(angle)
        .map_err(|e| RpcError::new(codes::UNAVAILABLE, e))?;
    // closed: the cover screen; otherwise the inner screen
    let cover = if posture == Posture::Closed {
        fold.cover
    } else {
        None
    };
    // AUD-P2-46: Android's posture first (it follows the hinge in tens of milliseconds) …
    let reported = wait_for_state(
        state.adb_port,
        serial,
        state_name(posture),
        Duration::from_millis(1500),
    );
    let state_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    // … then the screen: the cover screen when closed, the inner screen otherwise
    let had_cover = before.as_ref().is_some_and(|b| b.cover.is_some());
    match cover {
        Some((_, _, w, h)) if !had_cover => {
            umr_device::adb::shell(state.adb_port, serial, &format!("wm size {w}x{h}"))
                .map_err(|e| RpcError::new(codes::UNAVAILABLE, e.to_string()))?;
        }
        None if had_cover => {
            umr_device::adb::shell(state.adb_port, serial, "wm size reset")
                .map_err(|e| RpcError::new(codes::UNAVAILABLE, e.to_string()))?;
        }
        _ => {}
    }
    let f = FoldState {
        serial: serial.to_string(),
        avd: avd.to_string(),
        angle,
        posture,
        cover,
    };
    *state.fold.lock().unwrap_or_else(PoisonError::into_inner) = Some(f.clone());
    crate::preview_api::refresh_target(state);
    let mut v = describe(&f, fold);
    v["device_state"] = serde_json::json!(reported);
    v["state_ms"] = serde_json::json!(state_ms);
    v["ms"] = serde_json::json!(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX));
    tracing::info!(avd, angle, posture = posture_name(posture), "hinge");
    Ok(v)
}

/// A foldable emulator came up: open flat on its inner screen, whatever the snapshot held.
pub fn on_boot(
    state: &ApiState,
    serial: &str,
    avd: &str,
    grpc: Option<&umr_device::grpc::Emulator>,
) {
    let Some(fold) = profile_of_avd(avd).and_then(|p| p.fold.as_ref()) else {
        return;
    };
    if let Some(g) = grpc {
        let _ = g.set_hinge_angle(180.0);
    }
    if fold.cover.is_some() {
        let _ = umr_device::adb::shell(state.adb_port, serial, "wm size reset");
    }
    *state.fold.lock().unwrap_or_else(PoisonError::into_inner) = Some(FoldState {
        serial: serial.to_string(),
        avd: avd.to_string(),
        angle: 180.0,
        posture: Posture::Opened,
        cover: None,
    });
    crate::preview_api::refresh_target(state);
}

/// `device.posture`: with `angle` or `posture`, moves the hinge; with neither, reports the
/// hinge as it is (`{ angle, posture, label, cover, steps, kind }`).
pub fn posture(state: &ApiState, id: Id, p: &PostureParams) -> Response {
    // a bad request is refused before any device is looked at
    if p.angle
        .is_some_and(|a| !a.is_finite() || !(0.0..=180.0).contains(&a))
    {
        return Response::err(
            Some(id),
            RpcError::new(codes::INVALID_PARAMS, "the angle must be 0–180 degrees"),
        );
    }
    if let Some(name) = p.posture.as_deref().filter(|_| p.angle.is_none()) {
        if Posture::parse(name).is_none() {
            return Response::err(
                Some(id),
                RpcError::new(
                    codes::INVALID_PARAMS,
                    format!(
                        "unknown posture '{name}'; use closed, half-opened or opened, or an angle"
                    ),
                ),
            );
        }
    }
    let r = crate::app_api::target(state, p.target.as_deref()).and_then(|t| {
        let (Some(g), Some(avd)) = (&t.grpc, &t.avd) else {
            return Err(RpcError::new(
                codes::UNAVAILABLE,
                "hinge control needs an emulator started by the product (gRPC)",
            ));
        };
        let fold = profile_of_avd(avd)
            .and_then(|pr| pr.fold.as_ref())
            .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, format!("{avd} is not a foldable profile")))?;
        let angle = match (p.angle, p.posture.as_deref()) {
            (Some(a), _) if a.is_finite() => Some(a),
            (Some(_), _) => {
                return Err(RpcError::new(codes::INVALID_PARAMS, "the angle must be a number of degrees"))
            }
            (None, Some(name)) => {
                let want = Posture::parse(name).ok_or_else(|| {
                    RpcError::new(
                        codes::INVALID_PARAMS,
                        format!("unknown posture '{name}'; use closed, half-opened or opened, or an angle"),
                    )
                })?;
                if !fold.postures.contains(&want) {
                    return Err(RpcError::new(
                        codes::INVALID_PARAMS,
                        format!("{avd} does not support posture '{name}'"),
                    ));
                }
                Some(angle_of(want))
            }
            (None, None) => None,
        };
        if let Some(a) = angle {
            return set_angle(state, &t.serial, avd, g, a);
        }
        let f = current(state, &t.serial, avd).unwrap_or(FoldState {
            serial: t.serial.clone(),
            avd: avd.clone(),
            angle: 180.0,
            posture: Posture::Opened,
            cover: None,
        });
        Ok(describe(&f, fold))
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
    fn angles_map_to_the_avd_postures() {
        let fold = umr_device::catalog::find("fold")
            .unwrap()
            .fold
            .as_ref()
            .unwrap();
        for (a, want) in [
            (0.0, Posture::Closed),
            (29.9, Posture::Closed),
            (30.0, Posture::Closed),
            (31.0, Posture::HalfOpened),
            (60.0, Posture::HalfOpened),
            (90.0, Posture::HalfOpened),
            (120.0, Posture::HalfOpened),
            (150.0, Posture::HalfOpened),
            (151.0, Posture::Opened),
            (180.0, Posture::Opened),
        ] {
            assert_eq!(posture_at(fold, a), want, "{a}°");
        }
        assert_eq!(label(Posture::HalfOpened, 90.0), "Half open 90°");
        assert_eq!(label(Posture::Opened, 180.0), "Open (flat)");
        assert_eq!(label(Posture::Closed, 0.0), "Closed");
        assert_eq!(angle_of(Posture::HalfOpened), 90.0);
        assert_eq!(profile_of_avd("umr_flip_api36").unwrap().id, "flip");
    }
}
