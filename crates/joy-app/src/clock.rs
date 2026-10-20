// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Frame clock with bounded elapsed time.

use crate::app::FrameTime;

/// Default upper bound on one frame's elapsed time, in seconds; longer stalls do not make the simulation catch up.
pub const DEFAULT_MAXIMUM_FRAME_SECONDS: f32 = 0.1;

/// Wall-clock milliseconds since the Unix epoch (JavaScript's `Date.now()`), on native targets and in browsers.
///
/// Games seed unrepeatable sessions with it; seeded tests and captures pass fixed seeds instead. Returns zero if the
/// clock reads before the epoch.
pub fn unix_time_milliseconds() -> f64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map_or(0.0, |elapsed| (elapsed.as_secs_f64() * 1000.0).floor())
}

/// A monotonic stopwatch for CPU timings, natively and in browsers.
///
/// Readings measure wall time on the calling thread (including waits), never GPU execution.
#[derive(Clone, Copy, Debug)]
pub struct Stopwatch {
    started: web_time::Instant,
}

impl Stopwatch {
    /// Start timing now.
    pub fn start() -> Self {
        Self {
            started: web_time::Instant::now(),
        }
    }

    /// Milliseconds since [`start`](Self::start); never negative.
    pub fn elapsed_milliseconds(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }
}

/// Converts timestamps into clamped per-frame elapsed seconds.
#[derive(Clone, Debug)]
pub struct FrameClock {
    maximum_frame_seconds: f32,
    start_seconds: Option<f64>,
    previous_seconds: Option<f64>,
    frame_index: u64,
}

impl FrameClock {
    /// Create a clock whose elapsed time never exceeds `maximum_frame_seconds`.
    pub fn new(maximum_frame_seconds: f32) -> Self {
        Self {
            maximum_frame_seconds: maximum_frame_seconds.max(0.0),
            start_seconds: None,
            previous_seconds: None,
            frame_index: 0,
        }
    }

    /// Forget the previous timestamp so the next frame reports zero elapsed time (after pauses or visibility changes).
    pub fn reset(&mut self) {
        self.previous_seconds = None;
    }

    /// Advance to a timestamp in seconds and return this frame's timing.
    pub fn tick(&mut self, now_seconds: f64) -> FrameTime {
        let start = *self.start_seconds.get_or_insert(now_seconds);
        let elapsed = match self.previous_seconds {
            Some(previous) => ((now_seconds - previous).max(0.0) as f32).min(self.maximum_frame_seconds),
            None => 0.0,
        };
        self.previous_seconds = Some(now_seconds);
        let time = FrameTime {
            elapsed_seconds: elapsed,
            time_seconds: now_seconds - start,
            frame_index: self.frame_index,
        };
        self.frame_index += 1;
        time
    }
}

#[cfg(test)]
#[path = "clock.test.rs"]
mod tests;
