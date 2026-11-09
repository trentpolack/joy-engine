// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Vector helpers and closest-point and sweep tests for `f64` simulation: thresholded normalization, ground-plane
//! distance, point-to-segment and segment-to-segment distances (swept
//! spheres against capsules), a ray against an axis-aligned box, and a swept circle's earliest entry time.
//!
//! Apart from [`horizontal_distance`] (which uses the portable `hypot`), these are exact arithmetic, so they return
//! identical results on every target. Units
//! are whatever the caller's positions use; parameters along segments run from 0 at the start to 1 at the end.

use super::{
    DVec2,
    DVec3,
    clamp,
};

/// Vectors shorter than this have no reliable direction; [`normalize_or`] returns its fallback for them.
pub const NORMALIZE_MINIMUM_LENGTH: f64 = 1e-9;

/// Below this squared length a segment is treated as a point.
const DEGENERATE_LENGTH_SQUARED: f64 = 1e-12;

/// Below this magnitude a ray direction component is treated as parallel to that axis' slabs.
const PARALLEL_COMPONENT: f64 = 1e-12;

/// `vector` divided by its length, or `fallback` when it is shorter than [`NORMALIZE_MINIMUM_LENGTH`].
///
/// Unlike glam's `normalize_or`, this divides by the length (rather than multiplying by its reciprocal) and uses an
/// explicit threshold, so recorded simulations keep their exact results.
pub fn normalize_or(vector: DVec3, fallback: DVec3) -> DVec3 {
    let length = vector.length();
    if length < NORMALIZE_MINIMUM_LENGTH {
        return fallback;
    }
    vector / length
}

/// Ground-plane (XZ) distance between two Y-up points, ignoring height.
pub fn horizontal_distance(a: DVec3, b: DVec3) -> f64 {
    super::deterministic::hypot(a.x - b.x, a.z - b.z)
}

/// The closest point on a segment to a query point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentClosestPoint {
    /// Distance from the query point to `point`.
    pub distance: f64,
    /// Parameter of `point` along the segment, in `[0, 1]`.
    pub parameter: f64,
    /// The closest point on the segment.
    pub point: DVec3,
}

/// The closest approach of two segments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentApproach {
    /// Shortest distance between the segments.
    pub distance: f64,
    /// Parameter of the closest point along the first segment, in `[0, 1]`, so callers can locate a hit.
    pub first_parameter: f64,
}

/// Distance from `point` to the segment `start`→`end`, with the closest point and its parameter.
pub fn point_segment_distance(point: DVec3, start: DVec3, end: DVec3) -> SegmentClosestPoint {
    let segment = end - start;
    let length_squared = segment.dot(segment);
    let mut parameter = 0.0;
    if length_squared > DEGENERATE_LENGTH_SQUARED {
        parameter = clamp((point - start).dot(segment) / length_squared, 0.0, 1.0);
    }
    let closest = start + (segment * parameter);
    SegmentClosestPoint {
        distance: point.distance(closest),
        parameter,
        point: closest,
    }
}

/// Closest distance between segments `p0`→`p1` and `q0`→`q1` (for example a projectile's path against a capsule's
/// spine), and the parameter along the first segment.
pub fn segment_segment_distance(p0: DVec3, p1: DVec3, q0: DVec3, q1: DVec3) -> SegmentApproach {
    let d1 = p1 - p0;
    let d2 = q1 - q0;
    let r = p0 - q0;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);
    let mut s = 0.0;
    let mut t = 0.0;

    // Degenerate segments collapse to point tests.
    if a <= DEGENERATE_LENGTH_SQUARED && e <= DEGENERATE_LENGTH_SQUARED {
        return SegmentApproach {
            distance: p0.distance(q0),
            first_parameter: 0.0,
        };
    }
    if a <= DEGENERATE_LENGTH_SQUARED {
        t = clamp(f / e, 0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= DEGENERATE_LENGTH_SQUARED {
            s = clamp(-c / a, 0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denominator = (a * e) - (b * b);
            s = if denominator > DEGENERATE_LENGTH_SQUARED {
                clamp(((b * f) - (c * e)) / denominator, 0.0, 1.0)
            } else {
                0.0
            };
            t = ((b * s) + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = clamp(-c / a, 0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = clamp((b - c) / a, 0.0, 1.0);
            }
        }
    }

    let closest_p = p0 + (d1 * s);
    let closest_q = q0 + (d2 * t);
    SegmentApproach {
        distance: closest_p.distance(closest_q),
        first_parameter: s,
    }
}

/// Slab test for a ray from `origin` along `direction` against the axis-aligned box `minimum`..`maximum`.
///
/// Returns the entry distance in units of `direction` (zero when the origin starts inside the box), or `None` when the
/// ray misses or the box lies behind it.
pub fn ray_box_distance(origin: DVec3, direction: DVec3, minimum: DVec3, maximum: DVec3) -> Option<f64> {
    let mut near = f64::NEG_INFINITY;
    let mut far = f64::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < PARALLEL_COMPONENT {
            if origin[axis] < minimum[axis] || origin[axis] > maximum[axis] {
                return None;
            }
            continue;
        }
        let first = (minimum[axis] - origin[axis]) / direction[axis];
        let second = (maximum[axis] - origin[axis]) / direction[axis];
        near = f64::max(near, f64::min(first, second));
        far = f64::min(far, f64::max(first, second));
    }
    if far < f64::max(near, 0.0) {
        return None;
    }
    Some(f64::max(near, 0.0))
}

/// The parameter in `[0, 1]` at which the segment `start`→`end` first enters the circle at `center` with `radius`
/// (zero when it starts inside), or infinity when it misses.
///
/// Sweeping a moving circle's path against another circle with the summed radii gives the earliest contact, so fast
/// movers cannot tunnel through small targets or hit a farther one first.
pub fn segment_circle_entry(start: DVec2, end: DVec2, center: DVec2, radius: f64) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let offset_x = start.x - center.x;
    let offset_y = start.y - center.y;
    let c = (offset_x * offset_x) + (offset_y * offset_y) - (radius * radius);
    if c <= 0.0 {
        return 0.0;
    }
    let a = (dx * dx) + (dy * dy);
    if a == 0.0 {
        return f64::INFINITY;
    }
    let b = 2.0 * ((offset_x * dx) + (offset_y * dy));
    let discriminant = (b * b) - (4.0 * a * c);
    if discriminant < 0.0 {
        return f64::INFINITY;
    }
    let time = (-b - discriminant.sqrt()) / (2.0 * a);
    if (0.0..=1.0).contains(&time) { time } else { f64::INFINITY }
}

#[cfg(test)]
#[path = "geometry.test.rs"]
mod tests;
