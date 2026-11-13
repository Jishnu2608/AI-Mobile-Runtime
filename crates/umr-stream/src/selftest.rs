//! PRV-011 / NFR-02c: built-in latency self-test (POC-013 method).
//!
//! With Android's `pointer_location` overlay on (the caller switches it on and off), every touch
//! draws crosshair lines through the touched point. The test sends taps through the same gRPC
//! input stream the preview uses and times how long it takes until a streamed frame shows the
//! change at that point; a 2 s drag measures the frame rate. Taps go to the middle of the home
//! screen (the caller presses HOME first).

#![allow(clippy::many_single_char_names)] // image maths: w, h, x, y, a, b, r

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::watch;
use umr_device::grpc::{Emulator, InputEvent};

use crate::protocol::stream_size;
use crate::server::Target;
use crate::stats::{summarize, Summary};

#[derive(Debug, Clone, Serialize)]
pub struct SelfTest {
    pub taps: u32,
    /// Taps whose feedback showed up within 1.5 s.
    pub detected: u32,
    /// Touch sent → frame with the feedback received by the Core.
    pub latency: Summary,
    /// Frames per second while dragging.
    pub fps: f64,
    pub stream: (u32, u32),
}

/// Latest frame: (arrival, width, height, RGBA).
type Frame = Option<(Instant, u32, u32, Vec<u8>)>;

/// Whether `b` differs from `a` around (`x`, `y`) (stream pixels): > 1 % of the pixels in a box
/// of ±6 % of the width changed noticeably.
#[must_use]
pub fn changed_near(a: &[u8], b: &[u8], w: u32, h: u32, x: u32, y: u32) -> bool {
    if a.len() != b.len() || a.len() < (w * h * 4) as usize {
        return true; // size changed: count as a change
    }
    let r = (w * 6 / 100).max(4);
    let (x0, x1) = (x.saturating_sub(r), (x + r).min(w - 1));
    let (y0, y1) = (y.saturating_sub(r), (y + r).min(h - 1));
    let (mut changed, mut total) = (0u32, 0u32);
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            let i = ((yy * w + xx) * 4) as usize;
            let d: i32 = (0..3)
                .map(|c| (i32::from(a[i + c]) - i32::from(b[i + c])).abs())
                .sum();
            total += 1;
            if d > 60 {
                changed += 1;
            }
        }
    }
    changed * 100 > total
}

async fn next_frame(
    rx: &mut watch::Receiver<Frame>,
    timeout: Duration,
) -> Option<(Instant, u32, u32, Vec<u8>)> {
    tokio::time::timeout(timeout, rx.changed())
        .await
        .ok()?
        .ok()?;
    rx.borrow_and_update().clone()
}

/// Runs `taps` timed taps and a drag on `target`.
///
/// # Errors
/// The emulator's gRPC streams could not be opened or sent no frames.
#[allow(clippy::too_many_lines)] // the measurement reads top to bottom
pub async fn run(target: &Target, taps: u32) -> Result<SelfTest, String> {
    let emu = Emulator::new(target.grpc_port, target.grpc_token.clone());
    let size = stream_size((target.width, target.height), (540, 1200));
    let mut stream = emu.stream_screenshots(false, size.0, size.1).await?;
    let input = emu.input_stream().await?;
    let (tx, mut rx) = watch::channel::<Frame>(None);
    let reader = tokio::spawn(async move {
        while let Ok(Some(img)) = stream.message().await {
            let (w, h) = img.format.as_ref().map_or(size, |f| (f.width, f.height));
            let _ = tx.send(Some((Instant::now(), w, h, img.image)));
        }
    });
    let first = next_frame(&mut rx, Duration::from_secs(5)).await;
    if first.is_none() {
        reader.abort();
        return Err("the emulator sent no frames".into());
    }
    let to_dev = |v: u32, of: u32, dev: u32| -> i32 {
        i32::try_from(u64::from(v) * u64::from(dev) / u64::from(of.max(1))).unwrap_or(0)
    };
    let mut lat = Vec::new();
    for i in 0..taps {
        // settle, then take the baseline
        tokio::time::sleep(Duration::from_millis(350)).await;
        let base = rx.borrow_and_update().clone();
        let Some((_, w, h, base_px)) = base else {
            continue;
        };
        let sx = w * (25 + (i * 37) % 50) / 100;
        let sy = h * (40 + (i * 53) % 20) / 100;
        let (dx, dy) = (to_dev(sx, w, target.width), to_dev(sy, h, target.height));
        let sent = Instant::now();
        if input.send(InputEvent::touch(dx, dy, 1, 0)).await.is_err() {
            break;
        }
        let deadline = sent + Duration::from_millis(1500);
        while Instant::now() < deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some((at, fw, fh, px)) = next_frame(&mut rx, left).await else {
                break;
            };
            if (fw, fh) == (w, h) && changed_near(&base_px, &px, w, h, sx, sy) {
                lat.push(u64::try_from(at.duration_since(sent).as_micros()).unwrap_or(u64::MAX));
                break;
            }
        }
        let _ = input.send(InputEvent::touch(dx, dy, 0, 0)).await;
    }
    // frame rate while dragging for 2 s
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (w, h) = (target.width, target.height);
    let (x0, y) = (
        i32::try_from(w / 4).unwrap_or(0),
        i32::try_from(h / 2).unwrap_or(0),
    );
    let span = i32::try_from(w / 2).unwrap_or(1);
    let mut frames = 0u32;
    let start = Instant::now();
    let mut k = 0i32;
    let _ = input.send(InputEvent::touch(x0, y, 1, 0)).await;
    while start.elapsed() < Duration::from_secs(2) {
        k += 1;
        let x = x0 + (k * 12) % span;
        let _ = input.send(InputEvent::touch(x, y, 1, 0)).await;
        if next_frame(&mut rx, Duration::from_millis(8))
            .await
            .is_some()
        {
            frames += 1;
        }
    }
    let _ = input.send(InputEvent::touch(x0, y, 0, 0)).await;
    reader.abort();
    #[allow(clippy::cast_precision_loss)]
    let fps = (f64::from(frames) / start.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    Ok(SelfTest {
        taps,
        detected: u32::try_from(lat.len()).unwrap_or(0),
        latency: summarize(&lat),
        fps,
        stream: size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_change_only_near_the_point() {
        let (w, h) = (100u32, 100u32);
        let a = vec![0u8; (w * h * 4) as usize];
        let mut b = a.clone();
        // a crosshair through (50, 50)
        for i in 0..w {
            for p in [(i, 50), (50, i)] {
                let idx = ((p.1 * w + p.0) * 4) as usize;
                b[idx..idx + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        assert!(changed_near(&a, &b, w, h, 50, 50));
        assert!(!changed_near(&a, &a, w, h, 50, 50));
        // a change far away (a clock ticking in a corner) does not count
        let mut c = a.clone();
        for yy in 0..6 {
            for xx in 0..6 {
                let idx = ((yy * w + xx) * 4) as usize;
                c[idx..idx + 3].copy_from_slice(&[255, 0, 0]);
            }
        }
        assert!(!changed_near(&a, &c, w, h, 50, 50));
    }
}
