// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Engine math: the vector, matrix, and quaternion types every engine crate and app uses, scalar helpers, and
//! [`deterministic`] functions for simulations that must replay identically on every target.
//!
//! The vector, matrix, and quaternion types come from glam and are re-exported here so the engine owns the path:
//! engine crates and apps name `joy_core::math::Vec3` (`joy_core::math::Vec3` outside the engine), never `glam`
//! directly, so a future math library change touches only this module.
//!
//! Precision follows purpose:
//! - `f32` types ([`Vec2`], [`Vec3`], [`Vec4`], [`Quat`], [`Mat3`], [`Mat4`]) for rendering, physics bodies, cameras,
//!   and presentation.
//! - `f64` types ([`DVec2`], [`DVec3`], [`DMat4`]) for seeded simulation and authored geometry that must reproduce
//!   exactly.
//!
//! Arithmetic, `sqrt`, vector `length`, `distance`, and `normalize` are IEEE-exact and identical on every target.
//! Transcendental functions are not, so `f64` simulation code calls [`deterministic`] instead of the standard methods;
//! clippy enforces that in the simulation crates and games.

pub mod deterministic;

pub use glam::{
    DMat4,
    DVec2,
    DVec3,
    EulerRot,
    Mat3,
    Mat4,
    Quat,
    Vec2,
    Vec3,
    Vec4,
};

/// One full turn in radians.
pub const TAU: f64 = std::f64::consts::TAU;

/// Constrain a value to the inclusive range `[minimum, maximum]`.
///
/// Unlike `f64::clamp`, an inverted range does not panic: `maximum` wins, so authored ranges cannot abort a frame.
pub fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    minimum.max(maximum.min(value))
}

/// Hermite smoothstep between two edges; returns 0 below `minimum`, 1 above `maximum`, and a smooth ramp between.
pub fn smoothstep(minimum: f64, maximum: f64, value: f64) -> f64 {
    let normalized = clamp((value - minimum) / (maximum - minimum), 0.0, 1.0);
    normalized * normalized * (3.0 - (2.0 * normalized))
}

/// Distance from a rectangle's center to its edge along a ray.
///
/// `width` and `height` are positive logical units; `angle` is the ray direction in radians from +X. The result is in
/// the same logical units.
pub fn distance_to_rectangle_edge(width: f64, height: f64, angle: f64) -> f64 {
    let horizontal = (width / 2.0) / deterministic::cos(angle).abs();
    let vertical = (height / 2.0) / deterministic::sin(angle).abs();
    horizontal.min(vertical)
}

#[cfg(test)]
#[path = "../../tests/unit/math.rs"]
mod tests;
