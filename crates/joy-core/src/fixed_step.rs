// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Fixed-rate simulation inside variable-rate frames.
//!
//! A shell calls an app's phases once per displayed frame with that frame's elapsed seconds. Simulations whose rules
//! are tuned per fixed tick (velocities per tick, lifetimes in ticks) feed each frame's elapsed time to a
//! [`FixedStepLoop`] and run as many whole updates as it returns, so simulation speed never depends on the display's
//! refresh rate. Pair it with a shell frame limit of at least [`FixedStepLoop::MAXIMUM_FRAME_SECONDS`] so the shell's
//! own clamp does not discard time the loop would have simulated.

/// Accumulates frame time and reports how many fixed updates are due.
///
/// Owned by the simulation that steps it; holds only the leftover fraction of a step between frames. Times are seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct FixedStepLoop {
    step_seconds: f64,
    accumulated_seconds: f64,
}

impl Default for FixedStepLoop {
    fn default() -> Self {
        Self::new(Self::DEFAULT_UPDATES_PER_SECOND)
    }
}

impl FixedStepLoop {
    /// Fixed updates per second when a simulation does not choose a rate.
    pub const DEFAULT_UPDATES_PER_SECOND: f64 = 60.0;

    /// Longest frame, in seconds, that counts toward simulation; longer stalls do not make the simulation catch up.
    pub const MAXIMUM_FRAME_SECONDS: f64 = 0.25;

    /// Create a loop running `updates_per_second` fixed updates; non-positive or non-finite rates use the default.
    pub fn new(updates_per_second: f64) -> Self {
        let rate = if updates_per_second.is_finite() && updates_per_second > 0.0 {
            updates_per_second
        } else {
            Self::DEFAULT_UPDATES_PER_SECOND
        };
        Self { step_seconds: 1.0 / rate, accumulated_seconds: 0.0 }
    }

    /// Seconds simulated by one fixed update.
    pub fn step_seconds(&self) -> f64 {
        self.step_seconds
    }

    /// Add one frame's elapsed seconds (clamped to `[0, MAXIMUM_FRAME_SECONDS]`) and return how many fixed updates to
    /// run now, consuming them from the accumulator.
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        let elapsed =
            if elapsed_seconds.is_finite() { elapsed_seconds.clamp(0.0, Self::MAXIMUM_FRAME_SECONDS) } else { 0.0 };
        self.accumulated_seconds += elapsed;
        let mut updates = 0;
        while self.accumulated_seconds >= self.step_seconds {
            self.accumulated_seconds -= self.step_seconds;
            updates += 1;
        }
        updates
    }

    /// The remaining fraction of a step in `[0, 1)`, for interpolating presentation between updates.
    pub fn interpolation(&self) -> f64 {
        self.accumulated_seconds / self.step_seconds
    }

    /// Discard accumulated time, for example after a pause or a restart.
    pub fn reset(&mut self) {
        self.accumulated_seconds = 0.0;
    }
}

#[cfg(test)]
#[path = "fixed_step.test.rs"]
mod tests;
