// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Auto-repeat for held controls: a press fires once, then fires again at a steady rate once the control has been held
//! past a delay, the way text cursors and menu navigation repeat. Times are seconds.

/// Seconds a control must be held before it starts repeating.
pub const DEFAULT_REPEAT_DELAY_SECONDS: f64 = 0.5;

/// Seconds between repeats once repeating.
pub const DEFAULT_REPEAT_INTERVAL_SECONDS: f64 = 1.0 / 30.0;

/// Auto-repeat for one held control (a key, a button, or a stick direction past its threshold).
///
/// Owned by whoever samples the control; feed it the control's state and the frame's elapsed seconds every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyRepeat {
    delay_seconds: f64,
    interval_seconds: f64,
    // Seconds the control has been held, or `None` while released.
    held: Option<f64>,
}

impl Default for KeyRepeat {
    fn default() -> Self {
        Self::new(DEFAULT_REPEAT_DELAY_SECONDS, DEFAULT_REPEAT_INTERVAL_SECONDS)
    }
}

impl KeyRepeat {
    /// A repeat that starts after `delay_seconds` and then fires every `interval_seconds`. Non-finite or negative
    /// delays use the default, and intervals below a millisecond use one, so a repeat can never fire without bound.
    pub fn new(delay_seconds: f64, interval_seconds: f64) -> Self {
        let delay_seconds = if delay_seconds.is_finite() && delay_seconds >= 0.0 {
            delay_seconds
        } else {
            DEFAULT_REPEAT_DELAY_SECONDS
        };
        let interval_seconds = if interval_seconds.is_finite() { interval_seconds.max(1.0e-3) } else { 1.0e-3 };
        Self {
            delay_seconds,
            interval_seconds,
            held: None,
        }
    }

    /// Advance by `seconds` with the control's current state and return how many times it fires this frame: one on
    /// the press, none while waiting out the delay, then one per elapsed interval. Releasing resets it.
    pub fn update(&mut self, down: bool, seconds: f64) -> u32 {
        if !down {
            self.held = None;
            return 0;
        }
        let Some(previous) = self.held else {
            self.held = Some(0.0);
            return 1;
        };
        let held = previous + seconds.max(0.0);
        self.held = Some(held);
        self.repeats_by(held) - self.repeats_by(previous)
    }

    /// Whether the control is held.
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// Repeats owed after holding for `time` seconds, not counting the press.
    fn repeats_by(&self, time: f64) -> u32 {
        if time < self.delay_seconds {
            return 0;
        }
        ((time - self.delay_seconds) / self.interval_seconds) as u32 + 1
    }
}

#[cfg(test)]
#[path = "key_repeat.test.rs"]
mod tests;
