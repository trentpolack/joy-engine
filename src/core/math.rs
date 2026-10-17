// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Small scalar helpers shared by simulation, content, and rendering.
//!
//! Distances use [`js_hypot`], a port of V8's `Math.hypot`, rather than the platform `hypot`, so lengths (and every
//! threshold compared against them) match the TypeScript engine bit for bit on native and WebAssembly targets.

use serde::{
    Deserialize,
    Serialize,
};

use crate::core::random::RandomSource;
use crate::core::types::Vector2;

/// One full turn in radians.
pub const TAU: f64 = std::f64::consts::TAU;

/// Constrain a value to the inclusive range `[minimum, maximum]`.
pub fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    minimum.max(maximum.min(value))
}

/// Hermite smoothstep between two edges; returns 0 below `minimum`, 1 above `maximum`, and a smooth ramp between.
pub fn smoothstep(minimum: f64, maximum: f64, value: f64) -> f64 {
    let normalized = clamp((value - minimum) / (maximum - minimum), 0.0, 1.0);
    normalized * normalized * (3.0 - (2.0 * normalized))
}

/// Euclidean length of a list of components, computed exactly like V8's `Math.hypot`.
///
/// Values are normalized by the largest magnitude and summed with Kahan compensation before the square root, which
/// avoids overflow and reproduces JavaScript results bit for bit. Any infinite component yields infinity, otherwise
/// any NaN yields NaN; an empty list or all zeros yields 0.
pub fn js_hypot(values: &[f64]) -> f64 {
    let mut maximum: f64 = 0.0;
    let mut any_nan = false;
    for value in values {
        if value.is_nan() {
            any_nan = true;
            continue;
        }
        let magnitude = value.abs();
        if magnitude > maximum {
            maximum = magnitude;
        }
    }
    if maximum == f64::INFINITY {
        return f64::INFINITY;
    }
    if any_nan {
        return f64::NAN;
    }
    if maximum == 0.0 {
        return 0.0;
    }

    // Kahan summation of the normalized squares.
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in values {
        let normalized = value.abs() / maximum;
        let summand = (normalized * normalized) - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * maximum
}

/// Euclidean distance between two points in the caller's coordinate space (TypeScript `distanceBetween`).
pub fn distance_between(first: Vector2, second: Vector2) -> f64 {
    js_hypot(&[second.x - first.x, second.y - first.y])
}

/// A unit direction and the distance it was normalized by (TypeScript `directionBetween`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Direction2 {
    /// Horizontal unit component.
    pub x: f64,
    /// Vertical unit component.
    pub y: f64,
    /// Distance between the points, or 1 when they coincide.
    pub length: f64,
}

/// Return the unit direction from `first` to `second` and the distance between them.
///
/// Coincident positions (or a NaN distance) return a zero direction and a fallback length of 1, preserving a safe
/// divisor for callers that normalize using the returned length, exactly like the TypeScript `directionBetween`.
pub fn direction_between(first: Vector2, second: Vector2) -> Direction2 {
    let x = second.x - first.x;
    let y = second.y - first.y;
    let distance = js_hypot(&[x, y]);

    // JavaScript `length || 1`: zero and NaN fall back to one.
    let length = if distance == 0.0 || distance.is_nan() { 1.0 } else { distance };
    Direction2 { x: x / length, y: y / length, length }
}

/// Sample a floating-point value from the half-open range `[minimum, maximum)` using an injected stream.
pub fn random_range(minimum: f64, maximum: f64, random: &mut impl RandomSource) -> f64 {
    minimum + (random.next_unit() * (maximum - minimum))
}

/// Return an integer in `[minimum, maximum_exclusive)`; consumes exactly one draw, matching the TypeScript helper.
pub fn random_integer(minimum: i64, maximum_exclusive: i64, random: &mut impl RandomSource) -> i64 {
    random_range(minimum as f64, maximum_exclusive as f64, random).floor() as i64
}

/// Distance from a rectangle's center to its edge along a ray.
///
/// `width` and `height` are positive logical units; `angle` is the ray direction in radians from +X. The result is in
/// the same logical units.
pub fn distance_to_rectangle_edge(width: f64, height: f64, angle: f64) -> f64 {
    let horizontal = (width / 2.0) / angle.cos().abs();
    let vertical = (height / 2.0) / angle.sin().abs();
    horizontal.min(vertical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::random::SeededRandom;

    #[test]
    fn random_helpers_consume_one_draw() {
        let mut stream = SeededRandom::new(1);
        // First draw from seed 1 is 0.23645552527159452.
        assert_eq!(random_integer(0, 10, &mut stream), 2);
        let mut reference = SeededRandom::new(1);
        reference.next_unit();
        assert_eq!(stream, reference);
    }

    #[test]
    fn rectangle_edge_distance_selects_nearest_side() {
        assert!((distance_to_rectangle_edge(4.0, 2.0, 0.0) - 2.0).abs() < 1e-12);
        assert!((distance_to_rectangle_edge(4.0, 2.0, std::f64::consts::FRAC_PI_2) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn hypot_handles_special_values_like_javascript() {
        assert_eq!(js_hypot(&[]), 0.0);
        assert_eq!(js_hypot(&[3.0, 4.0]), 5.0);
        assert!(js_hypot(&[f64::NAN, 1.0]).is_nan());
        assert_eq!(js_hypot(&[f64::NAN, f64::NEG_INFINITY]), f64::INFINITY);
        assert_eq!(js_hypot(&[1e300, 1e300]), 1.4142135623730952e300);
        assert_eq!(
            direction_between(Vector2::new(1.0, 1.0), Vector2::new(1.0, 1.0)),
            Direction2 { x: 0.0, y: 0.0, length: 1.0 }
        );
    }
}
