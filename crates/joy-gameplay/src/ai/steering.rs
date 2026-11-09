// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Turn-limited steering for pursuers and homing projectiles.
//!
//! Planar helpers measure headings in radians from +X toward +Y (screen or arena space); the 3D helpers rotate unit
//! directions in Y-up world space. Turn limits are radians per call, so callers decide whether a call is one fixed tick
//! or a scaled frame.

use joy_core::math::{
    DVec2,
    DVec3,
    angle_difference,
    clamp,
    deterministic,
};

/// Below this axis length two directions are treated as parallel.
const PARALLEL_AXIS_LENGTH: f64 = 1e-8;

/// Below this angle a direction is treated as already on target.
const ON_TARGET_RADIANS: f64 = 1e-4;

/// Turn `heading` toward `target_heading` (radians) along the shorter arc by at most `maximum_turn` radians.
pub fn turn_heading_toward(heading: f64, target_heading: f64, maximum_turn: f64) -> f64 {
    let difference = angle_difference(heading, target_heading);
    heading + (-maximum_turn).max(maximum_turn.min(difference))
}

/// Turn a planar `velocity` at `position` toward `target` by at most `maximum_turn` radians, keeping its speed.
pub fn steer_velocity_toward(velocity: DVec2, position: DVec2, target: DVec2, maximum_turn: f64) -> DVec2 {
    let heading = deterministic::atan2(velocity.y, velocity.x);
    let target_heading = deterministic::atan2(target.y - position.y, target.x - position.x);
    let next_heading = turn_heading_toward(heading, target_heading, maximum_turn);
    let speed = deterministic::hypot(velocity.x, velocity.y);
    DVec2::new(deterministic::cos(next_heading) * speed, deterministic::sin(next_heading) * speed)
}

/// Rotate unit direction `from` toward unit direction `to` by `step` radians about their common normal (Rodrigues'
/// rotation), returning a unit direction. Parallel or opposite directions return `from` unchanged.
pub fn rotate_direction_toward(from: DVec3, to: DVec3, step: f64) -> DVec3 {
    let axis = from.cross(to);
    let axis_length = axis.length();
    if axis_length < PARALLEL_AXIS_LENGTH {
        return from;
    }
    let k = axis * (1.0 / axis_length);
    let cosine = deterministic::cos(step);
    let sine = deterministic::sin(step);
    let k_cross_v = k.cross(from);
    let k_dot_v = k.dot(from);
    let rotated = DVec3::new(
        (from[0] * cosine) + (k_cross_v[0] * sine) + (k[0] * k_dot_v * (1.0 - cosine)),
        (from[1] * cosine) + (k_cross_v[1] * sine) + (k[1] * k_dot_v * (1.0 - cosine)),
        (from[2] * cosine) + (k_cross_v[2] * sine) + (k[2] * k_dot_v * (1.0 - cosine)),
    );
    joy_core::math::normalize_or(rotated, DVec3::Z)
}

/// A seeker's next unit heading: turn `heading` toward `desired` by at most `maximum_step` radians, but only while the
/// target sits inside the `seeker_cone` half-angle (radians), so a target that dodges out of the cone is lost.
pub fn home_direction(heading: DVec3, desired: DVec3, seeker_cone: f64, maximum_step: f64) -> DVec3 {
    let angle = deterministic::acos(clamp(heading.dot(desired), -1.0, 1.0));
    if angle >= seeker_cone || angle <= ON_TARGET_RADIANS {
        return heading;
    }

    rotate_direction_toward(heading, desired, f64::min(angle, maximum_step))
}

#[cfg(test)]
#[path = "steering.test.rs"]
mod tests;
