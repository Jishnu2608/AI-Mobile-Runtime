//! Messages between the Core and the preview window.
//!
//! Core → window
//! * text `{"type":"hello", ...}` ([`Hello`]) after connecting and after a device switch;
//!   `{"type":"waiting"}` while no device runs.
//! * binary frame: `[0x03][width u32][height u32][seq u32][sent_us u64][RGB bytes]` (RGB888;
//!   3 bytes per pixel)
//!   (little-endian; `sent_us` = Core send time, Unix µs, so the window can time the hop).
//!
//! Window → Core (text JSON, coordinates in device pixels)
//! * `{"t":"touch","x":..,"y":..,"p":1|0,"id":0}` — pressure 0 = finger up
//! * `{"t":"key","key":"GoBack","e":"press"|"down"|"up"}`, `{"t":"text","text":".."}`
//! * `{"t":"wheel","dx":0,"dy":-120}`
//! * `{"t":"resize","w":..,"h":..}` — stream at this size (the window's canvas size)
//! * `{"t":"stats", "draw_p50":.., "draw_p95":.., "hop_p95":.., "fps":..}` — the window's timing

use serde::{Deserialize, Serialize};
use umr_device::grpc::InputEvent;

/// Binary frame header length.
pub const FRAME_HEADER: usize = 1 + 4 + 4 + 4 + 8;
pub const FRAME_KIND: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hello {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub serial: String,
    /// Device screen in pixels (input coordinates).
    pub device_width: u32,
    pub device_height: u32,
    /// Size the frames are streamed at.
    pub width: u32,
    pub height: u32,
    pub interact: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum ClientMsg {
    Touch {
        x: i32,
        y: i32,
        #[serde(default)]
        p: i32,
        #[serde(default)]
        id: i32,
    },
    Key {
        key: String,
        #[serde(default)]
        e: Option<String>,
    },
    Text {
        text: String,
    },
    Wheel {
        #[serde(default)]
        dx: i32,
        #[serde(default)]
        dy: i32,
    },
    Resize {
        w: u32,
        h: u32,
    },
    Stats(crate::stats::UiStats),
}

/// Keys the window may send (W3C names the emulator maps; anything else is refused).
pub const KEYS: &[&str] = &[
    "GoBack",
    "GoHome",
    "AppSwitch",
    "Power",
    "AudioVolumeUp",
    "AudioVolumeDown",
    "Enter",
    "Backspace",
    "Delete",
    "Tab",
    "Escape",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Shift",
    "Control",
    "Alt",
];

/// A client message as an emulator input event (`None`: not input, or refused).
#[must_use]
pub fn to_input(m: &ClientMsg, device: (u32, u32)) -> Option<InputEvent> {
    let clamp =
        |v: i32, max: u32| v.clamp(0, i32::try_from(max).unwrap_or(i32::MAX).saturating_sub(1));
    match m {
        ClientMsg::Touch { x, y, p, id } => Some(InputEvent::touch(
            clamp(*x, device.0),
            clamp(*y, device.1),
            i32::from(*p > 0),
            (*id).clamp(0, 9),
        )),
        ClientMsg::Key { key, e } => {
            // single printable characters are fine too (typing into the app)
            let ok = KEYS.contains(&key.as_str()) || key.chars().count() == 1;
            let t = match e.as_deref() {
                Some("down") => 0,
                Some("up") => 1,
                _ => 2,
            };
            ok.then(|| InputEvent::key(key, t))
        }
        ClientMsg::Text { text } => {
            (!text.is_empty() && text.len() <= 1000).then(|| InputEvent::text(text))
        }
        ClientMsg::Wheel { dx, dy } => Some(InputEvent::wheel(
            (*dx).clamp(-1200, 1200),
            (*dy).clamp(-1200, 1200),
        )),
        ClientMsg::Resize { .. } | ClientMsg::Stats(_) => None,
    }
}

/// Builds a binary frame.
#[must_use]
pub fn frame(width: u32, height: u32, seq: u32, sent_us: u64, rgba: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(FRAME_HEADER + rgba.len());
    v.push(FRAME_KIND);
    v.extend_from_slice(&width.to_le_bytes());
    v.extend_from_slice(&height.to_le_bytes());
    v.extend_from_slice(&seq.to_le_bytes());
    v.extend_from_slice(&sent_us.to_le_bytes());
    v.extend_from_slice(rgba);
    v
}

/// Stream size for a window canvas: the device aspect, at most the device size, even numbers.
#[must_use]
pub fn stream_size(device: (u32, u32), canvas: (u32, u32)) -> (u32, u32) {
    let (dw, dh) = (f64::from(device.0.max(1)), f64::from(device.1.max(1)));
    let (cw, ch) = (f64::from(canvas.0.max(64)), f64::from(canvas.1.max(64)));
    let scale = (cw / dw).min(ch / dh).min(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let even = |v: f64| ((v.round() as u32) / 2 * 2).max(2);
    (even(dw * scale), even(dh * scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn client_messages_become_input_events() {
        let m: ClientMsg = serde_json::from_str(r#"{"t":"touch","x":5000,"y":-3,"p":1}"#).unwrap();
        let e = to_input(&m, (1080, 2400)).unwrap();
        assert!(e.touch_event.is_some());
        let t = &e.touch_event.unwrap().touches[0];
        assert_eq!(
            (t.x, t.y, t.pressure),
            (1079, 0, 1),
            "clamped to the screen"
        );
        let k: ClientMsg = serde_json::from_str(r#"{"t":"key","key":"GoBack"}"#).unwrap();
        assert_eq!(
            to_input(&k, (1, 1)).unwrap().key_event.unwrap().key,
            "GoBack"
        );
        let bad: ClientMsg = serde_json::from_str(r#"{"t":"key","key":"LaunchMissiles"}"#).unwrap();
        assert!(to_input(&bad, (1, 1)).is_none());
        let r: ClientMsg = serde_json::from_str(r#"{"t":"resize","w":400,"h":800}"#).unwrap();
        assert!(to_input(&r, (1, 1)).is_none());
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"shell","cmd":"rm"}"#).is_err());
    }

    #[test]
    fn frame_layout_and_sizes() {
        let f = frame(2, 1, 7, 99, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(f.len(), FRAME_HEADER + 8);
        assert_eq!(f[0], FRAME_KIND);
        assert_eq!(u32::from_le_bytes(f[1..5].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(f[9..13].try_into().unwrap()), 7);
        assert_eq!(u64::from_le_bytes(f[13..21].try_into().unwrap()), 99);
        assert_eq!(stream_size((1080, 2400), (540, 2000)), (540, 1200));
        assert_eq!(
            stream_size((1080, 2400), (5000, 5000)),
            (1080, 2400),
            "never above device size"
        );
        assert_eq!(stream_size((1768, 2208), (400, 400)), (320, 400));
    }
}
