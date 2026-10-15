// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Small scalar helpers shared by simulation, content, and rendering.

use crate::random::RandomSource;

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
    use crate::random::SeededRandom;

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
}
