// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Solver-neutral synchronization boundary for resident collision shapes.
//!
//! Gameplay-visible mutation batches complete before [`PhysicsBackend::query`], avoiding asynchronous readback on aim
//! and character collision paths. GPU backends may retain simulation state on-device and publish only compact event and
//! readback records; CPU implementations remain the required fallback. [`PhysicsWorld`] is the engine's CPU backend:
//! resident shapes become static rapier boxes that dynamic bodies (for example debris) collide with.
//!
//! Backends apply mutations and step synchronously. Owned resources are released through `Drop`.

use indexmap::IndexSet;
use joy_core::math::Vec3;
use serde::{
    Deserialize,
    Serialize,
};

use super::{
    BodyOptions,
    PhysicsError,
    PhysicsWorld,
};
use joy_core::PrimitiveShape;
use joy_core::math::DVec3;

/// Where a backend keeps simulation state, serialized as `cpu` or `gpu`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhysicsResidency {
    /// State lives in host memory.
    Cpu,
    /// State lives on the GPU; queries read compact results back.
    Gpu,
}

/// An axis-aligned static box collider identified by a stable id (for example a destructible chunk id).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsCollisionShape {
    /// Stable, nonempty shape id.
    pub id: String,
    /// Box center in world units.
    pub center: DVec3,
    /// Box half extents in world units; positive.
    pub half_extents: DVec3,
}

/// Shape changes that take effect together at one simulation boundary. Removals apply before additions, so a batch can
/// replace a shape by removing and re-adding its id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsMutationBatch {
    /// Simulation boundary this batch belongs to (for example a [`ChunkChangeSet`](crate::ChunkChangeSet)
    /// revision). Backends reject batches older than the newest one they applied.
    pub sequence: u64,
    /// Ids of resident shapes to remove.
    pub removed_shape_ids: Vec<String>,
    /// Shapes to make resident.
    pub added_shapes: Vec<PhysicsCollisionShape>,
    /// Hint that cached contacts touching changed shapes are stale.
    pub invalidates_contacts: bool,
    /// Hint that spatial acceleration structures need a rebuild.
    pub rebuilds_acceleration_structure: bool,
}

/// Nearest resident shape hit by a backend query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsQueryHit {
    /// The hit shape's id.
    pub shape_id: String,
    /// World distance from the query origin.
    pub distance: f64,
    /// World-space hit point.
    pub point: DVec3,
}

/// Synchronization boundary for resident bodies, transforms, velocities, and shapes.
pub trait PhysicsBackend {
    /// Where this backend keeps its state.
    fn residency(&self) -> PhysicsResidency;

    /// Number of bodies the backend currently simulates or holds resident.
    fn active_body_count(&self) -> usize;

    /// Apply one batch atomically: on failure nothing changes.
    fn apply_mutations(&mut self, batch: &PhysicsMutationBatch) -> Result<(), PhysicsError>;

    /// Advance by finite, nonnegative elapsed seconds.
    fn step(&mut self, elapsed_seconds: f64) -> Result<(), PhysicsError>;

    /// Nearest resident shape hit by a world-space ray within `maximum_distance` world units, or `None`.
    fn query(&self, origin: DVec3, direction: DVec3, maximum_distance: f64) -> Option<PhysicsQueryHit>;
}

/// Create a contact-invalidating batch for chunk removals at one simulation boundary.
///
/// The batch adds nothing; it invalidates contacts and rebuilds acceleration structures only when something is
/// removed. Fails when any id is empty.
pub fn create_removal_mutation_batch(
    sequence: u64,
    removed_shape_ids: &[String],
) -> Result<PhysicsMutationBatch, PhysicsError> {
    if removed_shape_ids.iter().any(String::is_empty) {
        return Err(PhysicsError::InvalidMutationBatch);
    }
    let changed = !removed_shape_ids.is_empty();
    Ok(PhysicsMutationBatch {
        sequence,
        removed_shape_ids: removed_shape_ids.to_vec(),
        added_shapes: Vec::new(),
        invalidates_contacts: changed,
        rebuilds_acceleration_structure: changed,
    })
}

impl PhysicsBackend for PhysicsWorld {
    fn residency(&self) -> PhysicsResidency {
        PhysicsResidency::Cpu
    }

    /// Every live body, including resident shapes and bodies created directly through the world.
    fn active_body_count(&self) -> usize {
        self.body_count()
    }

    /// Remove listed shapes (unknown ids are ignored), then add new static boxes.
    ///
    /// Fails without changes for a batch older than the newest applied one, an empty or duplicate id, an id that stays
    /// resident, or a shape whose center or half extents are not finite (and positive) in `f32`. Rapier refreshes
    /// contacts and its broad phase itself, so the hint flags need no extra work.
    fn apply_mutations(&mut self, batch: &PhysicsMutationBatch) -> Result<(), PhysicsError> {
        self.prune_resident_shapes();
        if let Some(last) = self.last_mutation_sequence
            && batch.sequence < last
        {
            return Err(PhysicsError::StaleMutationBatch {
                sequence: batch.sequence,
                last,
            });
        }
        if batch.removed_shape_ids.iter().any(String::is_empty) {
            return Err(PhysicsError::InvalidMutationBatch);
        }

        // Validate every addition against the shapes that remain after this batch's removals.
        let removed: IndexSet<&str> = batch.removed_shape_ids.iter().map(String::as_str).collect();
        let mut added: IndexSet<&str> = IndexSet::new();
        let mut bodies = Vec::with_capacity(batch.added_shapes.len());
        for shape in &batch.added_shapes {
            if shape.id.is_empty() {
                return Err(PhysicsError::InvalidMutationBatch);
            }
            let stays_resident = self.resident_shapes.contains_key(&shape.id) && !removed.contains(shape.id.as_str());
            if stays_resident || !added.insert(shape.id.as_str()) {
                return Err(PhysicsError::DuplicateShape(shape.id.clone()));
            }
            bodies.push(static_box_options(shape)?);
        }

        for id in &batch.removed_shape_ids {
            if let Some(body) = self.resident_shapes.shift_remove(id) {
                self.remove_body(body);
            }
        }
        for (shape, options) in batch.added_shapes.iter().zip(bodies) {
            // Options were validated above, so creating a static box cannot fail.
            let body = self.create_body(options).expect("validated static boxes are always accepted");
            self.resident_shapes.insert(shape.id.clone(), body);
        }
        self.last_mutation_sequence = Some(batch.sequence);
        Ok(())
    }

    fn step(&mut self, elapsed_seconds: f64) -> Result<(), PhysicsError> {
        PhysicsWorld::step(self, elapsed_seconds as f32).map(|_| ())
    }

    /// Cast against resident shapes only. The direction is normalized; shapes containing the origin are skipped, like
    /// [`PhysicsWorld::raycast`]. Non-finite input, a zero direction, or a non-positive distance returns `None`.
    fn query(&self, origin: DVec3, direction: DVec3, maximum_distance: f64) -> Option<PhysicsQueryHit> {
        let from = origin.as_vec3();
        let direction = direction.as_vec3().normalize_or_zero();
        let maximum_distance = maximum_distance as f32;
        if direction == Vec3::ZERO || !maximum_distance.is_finite() || maximum_distance <= 0.0 {
            return None;
        }

        let to = from + (direction * maximum_distance);
        let resident = self.resident_shapes.values().filter(|body| body.world == self.world_id).map(|body| body.raw);
        let hit = self.raycast_bodies(from, to, resident)?;
        let shape_id = self.resident_shapes.iter().find(|(_, body)| **body == hit.body).map(|(id, _)| id.clone())?;
        Some(PhysicsQueryHit {
            shape_id,
            distance: f64::from(hit.distance),
            point: hit.point.as_dvec3(),
        })
    }
}

impl PhysicsWorld {
    /// Ids of the shapes currently resident through [`PhysicsBackend::apply_mutations`], in insertion order.
    pub fn resident_shape_ids(&self) -> impl Iterator<Item = &str> {
        self.resident_shapes.iter().filter(|(_, body)| self.contains_body(**body)).map(|(id, _)| id.as_str())
    }

    /// Forget resident shapes whose bodies were removed through [`PhysicsWorld::remove_body`].
    fn prune_resident_shapes(&mut self) {
        let stale: Vec<String> = self
            .resident_shapes
            .iter()
            .filter(|(_, body)| !self.contains_body(**body))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.resident_shapes.shift_remove(&id);
        }
    }
}

/// Validated static box options for a collision shape.
fn static_box_options(shape: &PhysicsCollisionShape) -> Result<BodyOptions, PhysicsError> {
    let center = shape.center.as_vec3();
    let half_extents = shape.half_extents.as_vec3();
    if !center.is_finite() || !half_extents.is_finite() || half_extents.min_element() <= 0.0 {
        return Err(PhysicsError::InvalidShape(shape.id.clone()));
    }
    Ok(BodyOptions {
        position: center,
        half_extents,
        ..BodyOptions::new(PrimitiveShape::Box)
    })
}
