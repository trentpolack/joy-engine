// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Fixed-rate simulation inside variable-rate frames.
//!
//! The shell calls a game's phases once per displayed frame with that frame's elapsed seconds. Games whose rules are
//! tuned per fixed tick (velocities per tick, lifetimes in ticks) feed each frame's elapsed time to a
//! [`FixedStepLoop`] and run as many whole updates as it returns, so simulation speed never depends on the display's
//! refresh rate. Pair it with `ShellOptions::maximum_frame_seconds` set to [`MAXIMUM_FRAME_SECONDS`] to bound
//! catch-up work after a stall.

/// Simulation updates per second when a game does not choose a rate (TypeScript `DEFAULT_UPDATE_RATE`).
pub const DEFAULT_UPDATES_PER_SECOND: f64 = 60.0;

/// Longest frame, in seconds, that counts toward simulation; longer stalls do not make the simulation catch up.
pub const MAXIMUM_FRAME_SECONDS: f64 = 0.25;

/// Accumulates frame time and reports how many fixed updates are due.
///
/// Owned by the game; holds only the leftover fraction of a step between frames.
#[derive(Clone, Debug, PartialEq)]
pub struct FixedStepLoop {
    step_seconds: f64,
    accumulated_seconds: f64,
}

impl Default for FixedStepLoop {
    fn default() -> Self {
        Self::new(DEFAULT_UPDATES_PER_SECOND)
    }
}

impl FixedStepLoop {
    /// Create a loop running `updates_per_second` fixed updates; non-positive or non-finite rates use the default.
    pub fn new(updates_per_second: f64) -> Self {
        let rate = if updates_per_second.is_finite() && updates_per_second > 0.0 {
            updates_per_second
        } else {
            DEFAULT_UPDATES_PER_SECOND
        };
        Self { step_seconds: 1.0 / rate, accumulated_seconds: 0.0 }
    }

    /// Seconds simulated by one fixed update.
    pub fn step_seconds(&self) -> f64 {
        self.step_seconds
    }

    /// Add one frame's elapsed seconds (clamped to `[0, MAXIMUM_FRAME_SECONDS]`) and return how many fixed updates to
    /// run now, consuming them from the accumulator exactly like the TypeScript loop.
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        let elapsed = if elapsed_seconds.is_finite() { elapsed_seconds.clamp(0.0, MAXIMUM_FRAME_SECONDS) } else { 0.0 };
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
mod tests {
    use super::*;

    #[test]
    fn frames_run_whole_fixed_updates_and_keep_the_remainder() {
        let mut game_loop = FixedStepLoop::default();
        assert_eq!(game_loop.advance(0.010), 0);
        assert_eq!(game_loop.advance(0.010), 1);
        assert!((game_loop.interpolation() - ((0.020 - (1.0 / 60.0)) * 60.0)).abs() < 1e-9);
        // A 30 Hz display runs two updates per frame; a long stall is limited to fifteen.
        game_loop.reset();
        assert_eq!(game_loop.advance(1.0 / 30.0), 2);
        game_loop.reset();
        assert_eq!(game_loop.advance(5.0), 15);
        assert_eq!(game_loop.advance(f64::NAN), 0);
        assert_eq!(FixedStepLoop::new(0.0), FixedStepLoop::default());
    }
}
