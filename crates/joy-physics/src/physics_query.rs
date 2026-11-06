// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Filtered scene queries: raycasts and sphere or box overlaps that respect collision groups, sensors, and an excluded
//! body.
//!
//! Queries test every live body against its current pose, so bodies created or teleported since the last step are found
//! immediately and results never depend on stale broad-phase data. Each candidate is first rejected by its bounding
//! box, which keeps the scan cheap at the body counts games use (hundreds); results are returned in body creation
//! order, so they are deterministic.

use joy_core::math::Vec3;
use rapier3d::dynamics::RigidBodyHandle;
use rapier3d::geometry::SharedShape;
use rapier3d::math::Pose;
use rapier3d::parry::bounding_volume::BoundingVolume;
use rapier3d::parry::query::intersection_test;

use super::convert::{
    finite_number,
    finite_vector,
    solver_rotation_from_euler,
    to_solver_vector,
};
use super::{
    BodyHandle,
    CollisionGroups,
    PhysicsError,
    PhysicsWorld,
    RaycastHit,
};

/// Which bodies a filtered query may return.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueryOptions {
    /// The querying layers: a body is returned only when these groups and the body's groups interact.
    pub groups: CollisionGroups,
    /// A body to skip, typically the one asking.
    pub exclude: Option<BodyHandle>,
    /// Whether sensor bodies may be returned; solid bodies are always eligible.
    pub include_sensors: bool,
}

impl QueryOptions {
    /// Every solid body, nothing excluded.
    pub const ALL_SOLID: Self = Self {
        groups: CollisionGroups::ALL,
        exclude: None,
        include_sensors: false,
    };
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self::ALL_SOLID
    }
}

impl PhysicsWorld {
    /// Closest hit along the world-space segment from `from` to `to` among bodies `options` admits, or `None`.
    ///
    /// Follows [`raycast`](Self::raycast)'s rules (a body containing the segment start is skipped; non-finite or
    /// zero-length segments return `None`).
    pub fn raycast_with(&self, from: Vec3, to: Vec3, options: &QueryOptions) -> Option<RaycastHit> {
        let candidates: Vec<RigidBodyHandle> = self.query_candidates(options).collect();
        self.raycast_bodies(from, to, candidates.into_iter())
    }

    /// Bodies whose collider overlaps a world-space sphere, in creation order.
    ///
    /// `radius` is meters and must be positive; non-finite input fails without side effects.
    pub fn overlap_sphere(
        &self,
        center: Vec3,
        radius: f32,
        options: &QueryOptions,
    ) -> Result<Vec<BodyHandle>, PhysicsError> {
        let center = finite_vector(center, "center")?;
        let radius = finite_number(radius, "radius", f64::from(f32::EPSILON), f64::INFINITY)?;
        let pose = Pose::from_translation(to_solver_vector(center));
        Ok(self.overlapping(&SharedShape::ball(radius), &pose, options))
    }

    /// Bodies whose collider overlaps a world-space box, in creation order.
    ///
    /// `half_extents` are meters (every component positive) and `rotation` is XYZ Euler radians; non-finite or
    /// non-positive input fails without side effects.
    pub fn overlap_box(
        &self,
        center: Vec3,
        half_extents: Vec3,
        rotation: Vec3,
        options: &QueryOptions,
    ) -> Result<Vec<BodyHandle>, PhysicsError> {
        let center = finite_vector(center, "center")?;
        let half_extents = finite_vector(half_extents, "halfExtents")?;
        let rotation = finite_vector(rotation, "rotation")?;
        if half_extents.min_element() <= 0.0 {
            return Err(PhysicsError::NonPositiveHalfExtents);
        }
        let pose = Pose::from_parts(to_solver_vector(center), solver_rotation_from_euler(rotation));
        let shape = SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z);
        Ok(self.overlapping(&shape, &pose, options))
    }

    /// Collision groups of a live body, or `None` for an unknown handle.
    pub fn body_groups(&self, handle: BodyHandle) -> Option<CollisionGroups> {
        let raw = self.resolve_body(handle)?;
        let collider = self.solver.colliders.get(self.bodies.get(&raw)?.collider)?;
        Some(CollisionGroups::from_solver(collider.collision_groups()))
    }

    /// Whether a live body is a sensor; `None` for an unknown handle.
    pub fn is_sensor(&self, handle: BodyHandle) -> Option<bool> {
        let raw = self.resolve_body(handle)?;
        let collider = self.solver.colliders.get(self.bodies.get(&raw)?.collider)?;
        Some(collider.is_sensor())
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------------------------------------------------

    /// Live bodies `options` admits, in creation order.
    fn query_candidates<'a>(&'a self, options: &'a QueryOptions) -> impl Iterator<Item = RigidBodyHandle> + 'a {
        let excluded = options.exclude.and_then(|handle| self.resolve_body(handle));
        self.bodies.iter().filter_map(move |(handle, record)| {
            if Some(*handle) == excluded {
                return None;
            }
            let collider = self.solver.colliders.get(record.collider)?;
            if collider.is_sensor() && !options.include_sensors {
                return None;
            }
            let groups = CollisionGroups::from_solver(collider.collision_groups());
            options.groups.interacts_with(groups).then_some(*handle)
        })
    }

    /// Admitted bodies whose collider overlaps `shape` at `pose`.
    ///
    /// Shapes the exact intersection test does not support (signed distance colliders) fall back to their bounding
    /// boxes, which may report a near miss as an overlap.
    fn overlapping(&self, shape: &SharedShape, pose: &Pose, options: &QueryOptions) -> Vec<BodyHandle> {
        let query_bounds = shape.compute_aabb(pose);
        let mut hits = Vec::new();
        for raw in self.query_candidates(options) {
            let Some(record) = self.bodies.get(&raw) else {
                continue;
            };
            let (Some(body), Some(collider)) =
                (self.solver.bodies.get(raw), self.solver.colliders.get(record.collider))
            else {
                continue;
            };
            let collider_pose = match collider.position_wrt_parent() {
                Some(local) => *body.position() * *local,
                None => *collider.position(),
            };
            let collider_bounds = collider.shape().compute_aabb(&collider_pose);
            if !query_bounds.intersects(&collider_bounds) {
                continue;
            }
            let overlapping = match intersection_test(pose, shape.as_ref(), &collider_pose, collider.shape()) {
                Ok(result) => result.intersecting,
                Err(_) => true,
            };
            if overlapping {
                hits.push(BodyHandle {
                    world: self.world_id,
                    raw,
                });
            }
        }
        hits
    }
}
