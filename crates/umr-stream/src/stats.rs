//! NFR-02a: time the product adds to the preview.
//!
//! * input: WebSocket message received → event handed to the gRPC input stream
//! * frame: gRPC frame received → WebSocket message queued
//! * window: reported by the preview window (frame received → drawn; Core → window hop)

use std::collections::VecDeque;
use std::time::Instant;

use serde::{Deserialize, Serialize};

const KEEP: usize = 2000;

/// Percentiles of a sample set (µs in, ms out).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Summary {
    pub samples: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

/// p50 / p95 / max of microsecond samples, in milliseconds.
#[must_use]
pub fn summarize(us: &[u64]) -> Summary {
    if us.is_empty() {
        return Summary::default();
    }
    let mut v = us.to_vec();
    v.sort_unstable();
    let at = |q: f64| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let i = ((v.len() as f64 - 1.0) * q).round() as usize;
        v[i.min(v.len() - 1)]
    };
    #[allow(clippy::cast_precision_loss)]
    let ms = |x: u64| (x as f64 / 10.0).round() / 100.0;
    Summary {
        samples: v.len(),
        p50_ms: ms(at(0.5)),
        p95_ms: ms(at(0.95)),
        max_ms: ms(*v.last().unwrap_or(&0)),
    }
}

/// What the preview window measured (sent every few seconds).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct UiStats {
    #[serde(default)]
    pub draw_p50: f64,
    #[serde(default)]
    pub draw_p95: f64,
    /// Core send → window receive (same machine clock).
    #[serde(default)]
    pub hop_p95: f64,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub frames: u64,
}

#[derive(Debug, Default)]
pub struct Stats {
    input_us: VecDeque<u64>,
    frame_us: VecDeque<u64>,
    frame_times: VecDeque<Instant>,
    pub frames: u64,
    pub inputs: u64,
    pub dropped: u64,
    pub ui: UiStats,
}

fn push(q: &mut VecDeque<u64>, v: u64) {
    q.push_back(v);
    while q.len() > KEEP {
        q.pop_front();
    }
}

impl Stats {
    pub fn input(&mut self, us: u64) {
        self.inputs += 1;
        push(&mut self.input_us, us);
    }

    pub fn frame(&mut self, us: u64) {
        self.frames += 1;
        push(&mut self.frame_us, us);
        self.frame_times.push_back(Instant::now());
        while self.frame_times.len() > 600 {
            self.frame_times.pop_front();
        }
    }

    /// Frames sent per second over the last 3 s.
    #[must_use]
    pub fn fps(&self) -> f64 {
        let now = Instant::now();
        let n = self
            .frame_times
            .iter()
            .filter(|t| now.duration_since(**t).as_secs_f64() <= 3.0)
            .count();
        #[allow(clippy::cast_precision_loss)]
        let f = n as f64 / 3.0;
        (f * 10.0).round() / 10.0
    }

    #[must_use]
    pub fn report(&self) -> Report {
        let input = summarize(&self.input_us.iter().copied().collect::<Vec<_>>());
        let frame = summarize(&self.frame_us.iter().copied().collect::<Vec<_>>());
        // NFR-02a: input forward + frame forward + window hop + window draw (p95 each, summed:
        // an upper bound)
        let pipeline = input.p95_ms + frame.p95_ms + self.ui.hop_p95 + self.ui.draw_p95;
        Report {
            input,
            frame,
            ui: self.ui,
            pipeline_p95_ms: (pipeline * 100.0).round() / 100.0,
            fps_sent: self.fps(),
            frames: self.frames,
            inputs: self.inputs,
            dropped: self.dropped,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Report {
    pub input: Summary,
    pub frame: Summary,
    pub ui: UiStats,
    /// NFR-02a: everything the product adds, p95 (target ≤ 20 ms).
    pub pipeline_p95_ms: f64,
    pub fps_sent: f64,
    pub frames: u64,
    pub inputs: u64,
    /// Frames skipped because the window was still busy with the previous one.
    pub dropped: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_in_ms() {
        let s = summarize(&(1..=100).map(|i| i * 1000).collect::<Vec<u64>>());
        assert_eq!(s.samples, 100);
        assert!((s.p50_ms - 50.0).abs() <= 1.0, "{s:?}");
        assert!((s.p95_ms - 95.0).abs() <= 1.0, "{s:?}");
        assert!((s.max_ms - 100.0).abs() < 0.01);
        assert_eq!(summarize(&[]), Summary::default());
    }

    #[test]
    fn pipeline_sums_our_parts() {
        let mut st = Stats::default();
        for _ in 0..10 {
            st.input(500);
            st.frame(2000);
        }
        st.ui = UiStats {
            draw_p50: 0.4,
            draw_p95: 0.8,
            hop_p95: 1.2,
            fps: 50.0,
            frames: 10,
        };
        let r = st.report();
        assert!(
            (r.pipeline_p95_ms - (0.5 + 2.0 + 1.2 + 0.8)).abs() < 0.01,
            "{r:?}"
        );
        assert!(r.fps_sent > 0.0);
    }
}
