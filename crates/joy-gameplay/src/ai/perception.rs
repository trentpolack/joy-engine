// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Deterministic perception queries over caller-owned snapshots: the nearest candidate within a range, and sight cones
//! for reticle lock-on and seeker heads.
//!
//! Queries take positions the caller has already filtered (alive, hostile, visible), so the engine never owns a game's
//! notion of factions or teams. Ties resolve to enumeration order.

use joy_core::math::{
    DVec2,
    DVec3,
    deterministic,
};

use super::BestChoice;

/// A position that perception queries can measure: 2D arenas (`DVec2`) and Y-up worlds (`DVec3`).
pub trait SpatialPoint: Copy {
    /// Straight-line distance to `other`, in the positions' units.
    fn distance_to(self, other: Self) -> f64;
}

impl SpatialPoint for DVec2 {
    fn distance_to(self, other: Self) -> f64 {
        self.distance(other)
    }
}

impl SpatialPoint for DVec3 {
    fn distance_to(self, other: Self) -> f64 {
        self.distance(other)
    }
}

/// The nearest candidate a query found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nearest<T, P> {
    /// The caller's candidate.
    pub candidate: T,
    /// The candidate's position.
    pub position: P,
    /// Distance from the query origin.
    pub distance: f64,
}

/// The candidate nearest `origin` strictly within `maximum_distance` (pass `f64::INFINITY` for no limit); the first
/// candidate wins ties.
pub fn nearest<T, P: SpatialPoint>(
    origin: P,
    candidates: impl IntoIterator<Item = (T, P)>,
    maximum_distance: f64,
) -> Option<Nearest<T, P>> {
    let mut choice = BestChoice::lowest_below(maximum_distance);
    for (candidate, position) in candidates {
        let distance = origin.distance_to(position);
        choice.offer((candidate, position, distance), distance);
    }
    choice.into_best().map(|(candidate, position, distance)| Nearest {
        candidate,
        position,
        distance,
    })
}

/// A point seen inside a [`SightCone`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConeSighting {
    /// Angle from the cone's axis, in radians within `[0, π/2)`.
    pub angle: f64,
    /// Distance from the cone's origin.
    pub distance: f64,
}

/// A viewing direction from an origin (a reticle ray, a seeker head, an eye), in Y-up world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SightCone {
    /// The apex.
    pub origin: DVec3,
    /// The axis; must be unit length.
    pub direction: DVec3,
}

impl SightCone {
    /// The angle and distance of `point` from the cone's axis, or `None` when it is level with or behind the origin.
    /// Compare the angle with a half-angle (radians) to test membership, and score lock-on candidates by it.
    pub fn sight(&self, point: DVec3) -> Option<ConeSighting> {
        let offset = point - self.origin;
        let distance = offset.length();
        let along = offset.dot(self.direction);
        if along <= 0.0 {
            return None;
        }
        Some(ConeSighting {
            angle: deterministic::acos(f64::min(1.0, along / distance)),
            distance,
        })
    }
}

#[cfg(test)]
#[path = "perception.test.rs"]
mod tests;
