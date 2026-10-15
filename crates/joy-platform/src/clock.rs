// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Frame clock with bounded elapsed time, matching the TypeScript `GameShell` timing rules.

use crate::game::FrameTime;

/// Default upper bound on one frame's elapsed time, in seconds; longer stalls do not make the simulation catch up.
pub const DEFAULT_MAXIMUM_FRAME_SECONDS: f32 = 0.1;

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
        let time =
            FrameTime { elapsed_seconds: elapsed, time_seconds: now_seconds - start, frame_index: self.frame_index };
        self.frame_index += 1;
        time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_is_bounded_and_resets() {
        let mut clock = FrameClock::new(DEFAULT_MAXIMUM_FRAME_SECONDS);
        assert_eq!(clock.tick(10.0).elapsed_seconds, 0.0);
        assert!((clock.tick(10.016).elapsed_seconds - 0.016).abs() < 1e-6);
        assert_eq!(clock.tick(15.0).elapsed_seconds, DEFAULT_MAXIMUM_FRAME_SECONDS);
        clock.reset();
        let resumed = clock.tick(20.0);
        assert_eq!(resumed.elapsed_seconds, 0.0);
        assert_eq!(resumed.frame_index, 3);
        assert!((resumed.time_seconds - 10.0).abs() < 1e-9);
    }
}
