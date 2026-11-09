// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Camera shake: a decaying shake level (trauma) turned into a smooth, deterministic offset by a few incommensurate
//! sine waves per axis.
//!
//! Games add or raise the level when something hits, advance it every frame (or simulation step), and add
//! [`CameraShake::offset`] to their camera's focus. The offset is built only from `math::deterministic` sines of the
//! shake's own clock, so shake never consumes gameplay or cosmetic random streams and captures reproduce exactly.
//! Offsets are in the caller's world units; frequencies are radians per unit of the clock the caller advances (seconds
//! for [`CameraShake::advance`], or any clock passed to [`CameraShake::offset_at`]).

use crate::math::{
    DVec3,
    clamp,
    deterministic,
};

/// One sine term: `sin(frequency * time + phase)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShakeWave {
    /// Radians per unit of shake time.
    pub frequency: f64,
    /// Radians added before taking the sine; `π/2` turns it into a cosine.
    pub phase: f64,
}

impl ShakeWave {
    /// A sine wave with no phase offset.
    pub const fn sine(frequency: f64) -> Self {
        Self {
            frequency,
            phase: 0.0,
        }
    }

    /// The wave's value at `time`, in `[-1, 1]`.
    pub fn sample(self, time: f64) -> f64 {
        deterministic::sin((time * self.frequency) + self.phase)
    }
}

/// How one axis moves: `gain * carrier(t) * modulator(t)`, scaled by the shake's response to its level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShakeAxis {
    /// Largest offset along the axis at full response, in world units (zero holds the axis still).
    pub gain: f64,
    /// The main oscillation.
    pub carrier: ShakeWave,
    /// An optional slower wave multiplied in, so the motion wanders instead of repeating.
    pub modulator: Option<ShakeWave>,
}

impl ShakeAxis {
    /// An axis that never moves.
    pub const STILL: Self = Self {
        gain: 0.0,
        carrier: ShakeWave::sine(0.0),
        modulator: None,
    };

    /// An axis driven by one sine wave.
    pub const fn wave(gain: f64, carrier: ShakeWave) -> Self {
        Self {
            gain,
            carrier,
            modulator: None,
        }
    }

    fn sample(self, time: f64) -> f64 {
        let modulation = self.modulator.map_or(1.0, |wave| wave.sample(time));
        self.carrier.sample(time) * modulation * self.gain
    }
}

/// How the shake level fades.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShakeDecay {
    /// Lose this much level per second (trauma that runs out at a steady rate).
    LinearPerSecond(f64),
    /// Multiply the level by `exp(-rate * seconds)` (a ring-down that never quite stops; see `rest_level`).
    ExponentialPerSecond(f64),
    /// Multiply the level by this factor once per [`CameraShake::advance`], whatever its duration, for simulations
    /// that already advance in fixed ticks.
    RetainedPerStep(f64),
}

/// How the level maps to offset amplitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShakeResponse {
    /// Amplitude follows the level.
    Linear,
    /// Amplitude follows the level squared, so small hits barely register and big ones read as impact.
    Squared,
}

/// A shake's tuning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraShakeSettings {
    /// How the level fades.
    pub decay: ShakeDecay,
    /// How the level becomes amplitude.
    pub response: ShakeResponse,
    /// [`CameraShake::add`] saturates here (`f64::INFINITY` for no limit).
    pub maximum_level: f64,
    /// At or below this level the shake is at rest: it offsets nothing and snaps to zero on the next advance.
    pub rest_level: f64,
    /// X, Y, and Z motion.
    pub axes: [ShakeAxis; 3],
}

/// Owns one camera's shake level and clock.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraShake {
    settings: CameraShakeSettings,
    level: f64,
    time: f64,
}

impl CameraShake {
    /// A shake at rest.
    pub fn new(settings: CameraShakeSettings) -> Self {
        Self {
            settings,
            level: 0.0,
            time: 0.0,
        }
    }

    /// The tuning.
    pub fn settings(&self) -> &CameraShakeSettings {
        &self.settings
    }

    /// The current level, before the response curve.
    pub fn level(&self) -> f64 {
        self.level
    }

    /// Add `amount` to the level, saturating within `[0, maximum_level]`, so repeated hits stack (trauma).
    pub fn add(&mut self, amount: f64) {
        self.level = clamp(self.level + amount, 0.0, self.settings.maximum_level);
    }

    /// Raise the level to at least `amount`, so the strongest recent hit sets the shake rather than their sum.
    pub fn raise_to(&mut self, amount: f64) {
        self.level = self.level.max(amount);
    }

    /// Stop shaking immediately; the clock keeps running so the next shake continues the same motion.
    pub fn clear(&mut self) {
        self.level = 0.0;
    }

    /// Advance the shake clock by `elapsed_seconds` and fade the level.
    pub fn advance(&mut self, elapsed_seconds: f64) {
        self.time += elapsed_seconds;
        if self.level <= self.settings.rest_level {
            self.level = 0.0;
            return;
        }
        self.level = match self.settings.decay {
            ShakeDecay::LinearPerSecond(rate) => (self.level - (rate * elapsed_seconds)).max(0.0),
            ShakeDecay::ExponentialPerSecond(rate) => self.level * deterministic::exp(-elapsed_seconds * rate),
            ShakeDecay::RetainedPerStep(retained) => self.level * retained,
        };
    }

    /// The offset for the current level at the shake's own clock, in world units.
    pub fn offset(&self) -> DVec3 {
        self.offset_at(self.time)
    }

    /// The offset for the current level at `time`, for callers that drive shake from another clock (a frame
    /// timestamp, for example). Zero at rest.
    pub fn offset_at(&self, time: f64) -> DVec3 {
        if self.level <= self.settings.rest_level {
            return DVec3::ZERO;
        }
        let amplitude = match self.settings.response {
            ShakeResponse::Linear => self.level,
            ShakeResponse::Squared => self.level * self.level,
        };
        let [x, y, z] = self.settings.axes;
        DVec3::new(x.sample(time), y.sample(time), z.sample(time)) * amplitude
    }
}

#[cfg(test)]
#[path = "camera_shake.test.rs"]
mod tests;
