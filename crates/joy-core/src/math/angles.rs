// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Angle and smoothing helpers for `f64` simulation and presentation: wrapping into one turn, the signed shortest
//! difference between two headings, turn-limited approach, and frame-rate independent exponential damping.
//!
//! Angles are radians. Transcendental calls go through [`deterministic`](super::deterministic), so seeded simulations
//! that steer or turn with these helpers replay identically on native and WebAssembly targets. Lua game scripts reach
//! the same functions as `wrap_angle`, `angle_difference`, `approach_angle`, and `damp_factor`.

use std::f64::consts::PI;

use super::deterministic;

/// Wrap `angle` (radians) into `(-π, π]` with a floating-point remainder, preserving values already in range exactly.
pub fn wrap_angle(angle: f64) -> f64 {
    let mut wrapped = angle % (PI * 2.0);
    if wrapped <= -PI {
        wrapped += PI * 2.0;
    }
    if wrapped > PI {
        wrapped -= PI * 2.0;
    }
    wrapped
}

/// The signed shortest rotation from heading `from` to heading `to`, in radians within `[-π, π]`.
///
/// Computed as `atan2(sin(to - from), cos(to - from))`, which tolerates arbitrarily large inputs; prefer it to
/// [`wrap_angle`] where a recorded simulation already depends on this form.
pub fn angle_difference(from: f64, to: f64) -> f64 {
    let difference = to - from;
    deterministic::atan2(deterministic::sin(difference), deterministic::cos(difference))
}

/// Rotate `current` toward `target` (radians) by at most `maximum_step` radians along the shorter arc, landing exactly
/// on `target` once it is within reach.
pub fn approach_angle(current: f64, target: f64, maximum_step: f64) -> f64 {
    let difference = wrap_angle(target - current);
    if difference.abs() <= maximum_step {
        return target;
    }
    current + (sign(difference) * maximum_step)
}

/// The exponential approach factor for a response `rate` (per second) over `elapsed_seconds`, in `[0, 1)` for
/// non-negative inputs. Multiply a remaining difference by it each frame for smoothing that does not depend on frame
/// rate: `value += (target - value) * damp_factor(rate, elapsed_seconds)`.
pub fn damp_factor(rate: f64, elapsed_seconds: f64) -> f64 {
    1.0 - deterministic::exp(-rate * elapsed_seconds)
}

/// Linear interpolation from `from` to `to` by `amount` (unclamped).
pub fn lerp(from: f64, to: f64, amount: f64) -> f64 {
    from + ((to - from) * amount)
}

/// The sign of `value` as -1, 0, or 1; zero (of either sign) and NaN return themselves, unlike `f64::signum`.
pub fn sign(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        value
    }
}

#[cfg(test)]
#[path = "angles.test.rs"]
mod tests;
