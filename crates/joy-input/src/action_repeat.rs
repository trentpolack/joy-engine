// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Auto-repeat timing for held actions: a press fires once, then the action fires again at a steady rate once it has
//! been held past a delay, the way text cursors and menu navigation repeat. Times are seconds.
//!
//! Private to the input manager, which runs one timer per action that declares an [`ActionRepeat`](crate::ActionRepeat)
//! and reports the result as [`ActionState::repeats`](crate::ActionState::repeats).

/// Shortest accepted repeat interval in seconds; shorter intervals would fire a held action without bound.
const MINIMUM_INTERVAL_SECONDS: f64 = 1.0e-3;

/// Repeat timing for one held action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RepeatTimer {
    delay_seconds: f64,
    interval_seconds: f64,
    // Seconds the action has been held, or `None` while released.
    held: Option<f64>,
}

impl RepeatTimer {
    /// A timer that starts repeating after `delay_seconds` and then fires every `interval_seconds`. Non-finite or
    /// negative delays use `default_delay_seconds`; intervals are at least a millisecond.
    pub(crate) fn new(delay_seconds: f64, interval_seconds: f64, default_delay_seconds: f64) -> Self {
        let delay_seconds =
            if delay_seconds.is_finite() && delay_seconds >= 0.0 { delay_seconds } else { default_delay_seconds };
        let interval_seconds = if interval_seconds.is_finite() {
            interval_seconds.max(MINIMUM_INTERVAL_SECONDS)
        } else {
            MINIMUM_INTERVAL_SECONDS
        };
        Self {
            delay_seconds,
            interval_seconds,
            held: None,
        }
    }

    /// Advance by `seconds` with the action's frame state and return how many times it fires this frame: one on a press
    /// (including a tap released before the frame), none while waiting out the delay, then one per elapsed interval.
    pub(crate) fn update(&mut self, down: bool, pressed: bool, seconds: f64) -> u32 {
        if pressed {
            self.held = down.then_some(0.0);
            return 1;
        }
        if !down {
            self.held = None;
            return 0;
        }

        // Held without a press edge this frame: keep counting from the previous hold, or start one.
        let Some(previous) = self.held else {
            self.held = Some(0.0);
            return 1;
        };
        let elapsed = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        let held = previous + elapsed;
        self.held = Some(held);
        self.repeats_by(held) - self.repeats_by(previous)
    }

    /// Forget the current hold, so the next frame the action is down counts as a new press.
    pub(crate) fn reset(&mut self) {
        self.held = None;
    }

    /// Repeats owed after holding for `time` seconds, not counting the press.
    fn repeats_by(&self, time: f64) -> u32 {
        if time < self.delay_seconds {
            return 0;
        }
        (((time - self.delay_seconds) / self.interval_seconds) as u32) + 1
    }
}
