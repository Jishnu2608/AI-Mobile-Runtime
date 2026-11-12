//! DEV-009: render health check.
//!
//! POC-006 finding 1: restoring a snapshot taken in a different (or broken) GPU session under
//! `-gpu host` gives black app windows while Android reports everything as resumed. Every
//! capture path shows the same black frame, so a raw `screencap` of a known screen (the
//! launcher, after HOME) tells a healthy session from a broken one.

use serde::Serialize;

/// Result of checking one raw `screencap` frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Health {
    pub width: u32,
    pub height: u32,
    /// Share of sampled pixels in the content area that are not (near) black.
    pub lit: f64,
    /// Distinct colours among the samples (4 bits per channel).
    pub colours: usize,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// Minimum share of lit pixels for a healthy launcher screen.
pub const MIN_LIT: f64 = 0.02;
/// Minimum distinct colours (a uniform frame is not a rendered launcher).
pub const MIN_COLOURS: usize = 4;

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn bad(problem: &str) -> Health {
    Health {
        problem: Some(problem.to_string()),
        ..Health::default()
    }
}

/// Checks a raw `screencap` (no `-p`): header `width, height, format[, colour space]` (u32 LE)
/// followed by 4-byte pixels. The status bar (top 6 %) and the navigation bar (bottom 12 %) are
/// left out: in the broken state they are the only parts that still render.
#[must_use]
#[allow(clippy::many_single_char_names)] // w/h/i/r/g/b/p: image maths
pub fn assess(raw: &[u8]) -> Health {
    let (Some(w), Some(h)) = (u32_at(raw, 0), u32_at(raw, 4)) else {
        return bad("screencap output too short");
    };
    let pixels = (w as usize).saturating_mul(h as usize);
    if pixels == 0 || pixels > 64 * 1024 * 1024 {
        return bad("screencap size is not plausible");
    }
    let Some(header) = raw.len().checked_sub(pixels * 4) else {
        return bad("screencap output is truncated");
    };
    if header != 12 && header != 16 {
        return bad("unexpected screencap format");
    }
    assess_pixels(w, h, &raw[header..])
}

/// Checks `w × h` RGBA pixels (gRPC `RGBA8888` screenshot or a screencap body).
#[must_use]
#[allow(clippy::many_single_char_names)] // w/h/i/r/g/b/p: image maths
pub fn assess_pixels(w: u32, h: u32, body: &[u8]) -> Health {
    let pixels = (w as usize).saturating_mul(h as usize);
    if pixels == 0 || body.len() < pixels * 4 {
        return bad("frame size does not match its pixels");
    }
    let (w, h) = (w as usize, h as usize);
    let (top, bottom) = (h * 6 / 100, h * 88 / 100);
    let step = (((bottom - top) * w) / 40_000).max(1);
    let (mut seen, mut lit) = (0usize, 0usize);
    let mut colours = std::collections::HashSet::new();
    let mut i = top * w;
    while i < bottom * w {
        let p = &body[i * 4..i * 4 + 4];
        let (r, g, b) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]));
        seen += 1;
        if (r * 3 + g * 6 + b) / 10 > 24 {
            lit += 1;
        }
        if colours.len() < 1024 {
            colours.insert((p[0] >> 4, p[1] >> 4, p[2] >> 4));
        }
        i += step;
    }
    #[allow(clippy::cast_precision_loss)] // sample counts are far below 2^52
    let lit = if seen == 0 {
        0.0
    } else {
        lit as f64 / seen as f64
    };
    let ok = lit >= MIN_LIT && colours.len() >= MIN_COLOURS;
    Health {
        width: u32::try_from(w).unwrap_or(0),
        height: u32::try_from(h).unwrap_or(0),
        lit: (lit * 1000.0).round() / 1000.0,
        colours: colours.len(),
        ok,
        problem: (!ok).then(|| {
            "the screen is black although Android reports the system as running (GPU / snapshot mismatch)".to_string()
        }),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A raw frame: 16-byte header, then `f(x, y)` as RGBA.
    pub fn frame(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut v = Vec::new();
        for n in [w, h, 1, 0] {
            v.extend_from_slice(&n.to_le_bytes());
        }
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&f(x, y));
            }
        }
        v
    }

    #[test]
    fn launcher_is_healthy_and_black_window_is_not() {
        // wallpaper-like gradient
        #[allow(clippy::cast_possible_truncation)]
        let good = frame(108, 240, |x, y| [(x * 2) as u8, (y) as u8, 120, 255]);
        let g = assess(&good);
        assert!(g.ok, "{g:?}");
        assert_eq!((g.width, g.height), (108, 240));

        // black content area, only the navigation bar renders (POC-006 finding 1)
        let black = frame(108, 240, |_, y| {
            if y > 225 {
                [200, 200, 200, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let b = assess(&black);
        assert!(!b.ok && b.lit < 0.01, "{b:?}");
        assert!(b.problem.is_some());

        // uniform grey is not a rendered launcher either
        assert!(!assess(&frame(50, 50, |_, _| [128, 128, 128, 255])).ok);
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn rejects_malformed_output() {
        assert!(!assess(b"short").ok);
        let mut f = frame(10, 10, |_, _| [255, 0, 0, 255]);
        f.truncate(100);
        assert!(assess(&f).problem.unwrap().contains("truncated"));
        // old 12-byte header is accepted
        let mut old = frame(20, 20, |x, y| [(x * 10) as u8, (y * 10) as u8, 50, 255]);
        old.drain(12..16);
        assert!(assess(&old).ok);
    }
}
