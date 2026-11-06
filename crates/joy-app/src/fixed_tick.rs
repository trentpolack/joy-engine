// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The shells' fixed simulation tick: how many [`App::fixed_update`](crate::App::fixed_update) calls each displayed
//! frame owes, and how far presentation is toward the next one.
//!
//! Both the windowed and headless shells own one ticker, so a game's simulation runs at the same rate (and with the same
//! results) whatever the display refreshes at.

use joy_core::FixedStepLoop;

/// One fixed simulation tick handed to [`App::fixed_update`](crate::App::fixed_update).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedTick {
    /// Seconds one tick simulates (the reciprocal of `ShellOptions::fixed_updates_per_second`).
    pub step_seconds: f64,
    /// Zero-based count of ticks the shell has run; clock resets do not rewind it.
    pub index: u64,
}

/// Accumulates frame time into whole fixed ticks.
#[derive(Clone, Debug)]
pub(crate) struct FixedTicker {
    accumulator: FixedStepLoop,
    next_index: u64,
}

impl FixedTicker {
    /// A ticker running `updates_per_second` ticks; non-positive or non-finite rates use the engine default (60).
    pub(crate) fn new(updates_per_second: f64) -> Self {
        Self {
            accumulator: FixedStepLoop::new(updates_per_second),
            next_index: 0,
        }
    }

    /// Add one frame's elapsed seconds and return how many ticks are now due.
    pub(crate) fn advance(&mut self, elapsed_seconds: f32) -> u32 {
        self.accumulator.advance(f64::from(elapsed_seconds))
    }

    /// The next tick to run, counting it as run.
    pub(crate) fn next_tick(&mut self) -> FixedTick {
        let tick = FixedTick {
            step_seconds: self.accumulator.step_seconds(),
            index: self.next_index,
        };
        self.next_index += 1;
        tick
    }

    /// Fraction of a tick accumulated toward the next one, in `[0, 1)`.
    pub(crate) fn interpolation(&self) -> f32 {
        self.accumulator.interpolation() as f32
    }

    /// Discard accumulated time after a pause, visibility change, or clock reset.
    pub(crate) fn reset(&mut self) {
        self.accumulator.reset();
    }
}
