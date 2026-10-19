// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Damage, support, and static queries for a structure of stable axis-aligned chunks.
//!
//! Radial damage is queued and committed only at an explicit simulation boundary, so geometry, collision, and queries
//! change together. Coordinates and dimensions use caller-defined world units (Y up); hit points use the same units as
//! queued damage. Arithmetic uses `f64` in the established evaluation order to preserve recorded parity results.

use indexmap::IndexMap;
use serde::{
    Deserialize,
    Serialize,
};

use super::DestructionError;
use super::chunk_support::{
    ChunkSupportGraph,
    ChunkSupportOptions,
    ChunkSupportState,
};
use joy_core::math::DVec3;

/// Direction components smaller than this are treated as parallel to a slab in ray tests.
const PARALLEL_EPSILON: f64 = 1e-9;

/// One authored chunk: a stable id, an axis-aligned box, a material name, and its starting hit points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestructibleChunkDefinition {
    /// Stable, nonempty id, unique within a structure.
    pub id: String,
    /// Box center in world units.
    pub center: DVec3,
    /// Full box dimensions in world units; every component positive.
    pub size: DVec3,
    /// Nonempty material name (presentation looks it up; `frame` and `interior` have built-in chunk tints).
    pub material: String,
    /// Starting hit points; positive.
    pub hit_points: f64,
}

/// Damage applied to every active chunk whose box intersects a sphere.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadialDamage {
    /// Sphere center in world units; finite.
    pub center: DVec3,
    /// Sphere radius in world units; finite and positive.
    pub radius: f64,
    /// Hit points removed from each intersected chunk; finite and positive.
    pub damage: f64,
}

/// Chunks that left the structure at one commit or reset boundary. Records are owned copies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkChangeSet {
    /// Structure revision after this boundary; increments when chunks leave, and on every reset.
    pub revision: u64,
    /// All chunks leaving static geometry and queries at this boundary, including support detachments: impact
    /// removals first, then detachments, each in generation order.
    pub removed: Vec<DestructibleChunkDefinition>,
    /// Subset of `removed` released by support loss, retaining their remaining health.
    pub detached: Vec<DestructibleChunkDefinition>,
    /// Active chunks after this boundary.
    pub active_chunk_count: usize,
}

/// Copied committed damage state for one chunk. Hit points use the same units as queued damage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkDamageState {
    /// Stable chunk id.
    pub chunk_id: String,
    /// Remaining hit points, never below zero. Support-detached chunks keep their health.
    pub remaining_hit_points: f64,
    /// Starting hit points.
    pub maximum_hit_points: f64,
    /// Whether the chunk is still part of the structure.
    pub active: bool,
}

/// Nearest active chunk hit by a ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChunkRaycastHit<'a> {
    /// The borrowed chunk definition.
    pub chunk: &'a DestructibleChunkDefinition,
    /// Ray parameter of the entry point: world distance for a unit direction. Zero when the origin is inside.
    pub distance: f64,
    /// World-space entry point, `origin + direction * distance`.
    pub point: DVec3,
}

/// Mutable damage state for one chunk.
#[derive(Clone, Debug, PartialEq)]
struct ChunkState {
    definition: DestructibleChunkDefinition,
    hit_points: f64,
    active: bool,
}

/// Owns damage for stable, axis-aligned chunks and commits queued changes only at an explicit simulation boundary.
///
/// Definitions are copied at construction, so generation data can be released or reused. Chunks keep generation order
/// for every query and report. Without support options, chunks leave only when their hit points reach zero; with
/// support, [`ChunkStructure::commit`] also detaches chunks that lose their route to a ground anchor.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkStructure {
    // Chunks by id, in generation order.
    chunks: IndexMap<String, ChunkState>,

    // Damage waiting for the next commit.
    queued_damage: Vec<RadialDamage>,
    revision: u64,

    // Optional support topology and the diagnostics of the latest evaluation.
    support: Option<ChunkSupportGraph>,
    support_snapshot: Vec<ChunkSupportState>,
}

impl ChunkStructure {
    /// Copy and validate definitions, then evaluate initial support when `support` is provided.
    ///
    /// Fails for an empty id or material, non-finite coordinates, non-positive dimensions or hit points, a duplicate
    /// id, or invalid support options.
    pub fn new(
        definitions: &[DestructibleChunkDefinition],
        support: Option<ChunkSupportOptions>,
    ) -> Result<Self, DestructionError> {
        let mut chunks = IndexMap::with_capacity(definitions.len());
        for definition in definitions {
            validate_definition(definition)?;
            if chunks.contains_key(&definition.id) {
                return Err(DestructionError::DuplicateChunk(definition.id.clone()));
            }
            let state = ChunkState { definition: definition.clone(), hit_points: definition.hit_points, active: true };
            chunks.insert(definition.id.clone(), state);
        }

        let support = match support {
            None => None,
            Some(options) => {
                let all: Vec<&DestructibleChunkDefinition> = chunks.values().map(|chunk| &chunk.definition).collect();
                Some(ChunkSupportGraph::new(&all, options)?)
            }
        };
        let mut structure =
            Self { chunks, queued_damage: Vec::new(), revision: 0, support, support_snapshot: Vec::new() };
        structure.refresh_support();
        Ok(structure)
    }

    /// Queue radial damage. Geometry, collision, and diagnostics stay unchanged until [`ChunkStructure::commit`].
    pub fn queue_damage(&mut self, damage: RadialDamage) -> Result<(), DestructionError> {
        if !damage.center.is_finite() {
            return Err(DestructionError::InvalidDamageCenter);
        }
        let positive = |value: f64| value.is_finite() && value > 0.0;
        if !positive(damage.radius) || !positive(damage.damage) {
            return Err(DestructionError::InvalidDamageAmount);
        }
        self.queued_damage.push(damage);
        Ok(())
    }

    /// Apply every queued hit atomically and return the chunks removed at this boundary.
    ///
    /// Overlapping hits accumulate before removal, and already removed chunks are ignored. With support enabled, the
    /// surviving contacts are evaluated once and unsupported chunks detach in the same revision; the support
    /// diagnostics then describe that evaluation (detached chunks read as unsupported).
    pub fn commit(&mut self) -> ChunkChangeSet {
        let queued = std::mem::take(&mut self.queued_damage);
        for damage in &queued {
            for chunk in self.chunks.values_mut() {
                if chunk.active && sphere_intersects_box(damage.center, damage.radius, &chunk.definition) {
                    chunk.hit_points -= damage.damage;
                }
            }
        }

        let mut removed = Vec::new();
        for chunk in self.chunks.values_mut() {
            if chunk.active && chunk.hit_points <= 0.0 {
                chunk.active = false;
                removed.push(chunk.definition.clone());
            }
        }

        self.refresh_support();
        let mut detached = Vec::new();
        for state in &self.support_snapshot {
            let Some(chunk) = self.chunks.get_mut(&state.chunk_id) else {
                continue;
            };
            if chunk.active && !state.supported {
                chunk.active = false;
                detached.push(chunk.definition.clone());
                removed.push(chunk.definition.clone());
            }
        }

        if !removed.is_empty() {
            self.revision += 1;
        }
        ChunkChangeSet { revision: self.revision, removed, detached, active_chunk_count: self.active_chunk_count() }
    }

    /// Restore original health and activity, discarding uncommitted hits, and re-evaluate support.
    ///
    /// The revision always increments. The returned change set lists no removals; every chunk is active again.
    pub fn reset(&mut self) -> ChunkChangeSet {
        self.queued_damage.clear();
        for chunk in self.chunks.values_mut() {
            chunk.active = true;
            chunk.hit_points = chunk.definition.hit_points;
        }
        self.revision += 1;
        self.refresh_support();
        ChunkChangeSet {
            revision: self.revision,
            removed: Vec::new(),
            detached: Vec::new(),
            active_chunk_count: self.chunks.len(),
        }
    }

    /// Borrowed active definitions in stable generation order.
    pub fn active_chunks(&self) -> impl Iterator<Item = &DestructibleChunkDefinition> + '_ {
        self.chunks.values().filter(|chunk| chunk.active).map(|chunk| &chunk.definition)
    }

    /// Number of active chunks.
    pub fn active_chunk_count(&self) -> usize {
        self.chunks.values().filter(|chunk| chunk.active).count()
    }

    /// Total number of chunks, active or not.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Current structure revision (see [`ChunkChangeSet::revision`]).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a chunk with this id is still part of the structure.
    pub fn is_active(&self, chunk_id: &str) -> bool {
        self.chunks.get(chunk_id).is_some_and(|chunk| chunk.active)
    }

    /// Copy committed damage state in generation order. Queued damage is invisible until commit.
    ///
    /// Impact-destroyed chunks report zero health; support-detached chunks keep their health while becoming inactive.
    pub fn damage_state(&self) -> Vec<ChunkDamageState> {
        self.chunks
            .values()
            .map(|chunk| ChunkDamageState {
                chunk_id: chunk.definition.id.clone(),
                remaining_hit_points: chunk.hit_points.max(0.0),
                maximum_hit_points: chunk.definition.hit_points,
                active: chunk.active,
            })
            .collect()
    }

    /// Copy the committed static support diagnostics, or an empty list when support is disabled.
    pub fn support_state(&self) -> Vec<ChunkSupportState> {
        self.support_snapshot.clone()
    }

    /// Nearest active chunk hit by a world-space ray within `maximum_distance`, or `None`.
    ///
    /// `direction` need not be unit length; distances are ray parameters. A ray starting inside a chunk hits it at
    /// distance zero. Ties keep the earliest chunk in generation order. Fails for non-finite vectors or a maximum
    /// distance that is not finite and positive.
    pub fn raycast(
        &self,
        origin: DVec3,
        direction: DVec3,
        maximum_distance: f64,
    ) -> Result<Option<ChunkRaycastHit<'_>>, DestructionError> {
        let finite = origin.is_finite() && direction.is_finite();
        if !finite || !maximum_distance.is_finite() || maximum_distance <= 0.0 {
            return Err(DestructionError::InvalidRaycast);
        }

        let mut nearest: Option<ChunkRaycastHit<'_>> = None;
        for chunk in self.active_chunks() {
            let Some(distance) = ray_box_distance(origin, direction, chunk) else {
                continue;
            };
            if distance > maximum_distance || nearest.is_some_and(|hit| distance >= hit.distance) {
                continue;
            }
            let point = origin + (direction * distance);
            nearest = Some(ChunkRaycastHit { chunk, distance, point });
        }
        Ok(nearest)
    }

    /// Whether an axis-aligned probe (for example a player's collision box) fits at `center` without overlapping any
    /// active chunk. Touching faces do not overlap. Fails for a non-finite center or non-positive half extents.
    pub fn can_occupy(&self, center: DVec3, half_extents: DVec3) -> Result<bool, DestructionError> {
        let finite_center = center.is_finite();
        let positive_extents = half_extents.is_finite() && half_extents.cmpgt(DVec3::ZERO).all();
        if !finite_center || !positive_extents {
            return Err(DestructionError::InvalidOccupancyQuery);
        }
        let blocked = self.active_chunks().any(|chunk| {
            let chunk_half_extents = chunk.size.map(|value| value / 2.0);
            boxes_overlap(center, half_extents, chunk.center, chunk_half_extents)
        });
        Ok(!blocked)
    }

    /// Re-evaluate support for the current active set (no-op diagnostics when support is disabled).
    fn refresh_support(&mut self) {
        self.support_snapshot = match &self.support {
            None => Vec::new(),
            Some(graph) => {
                let active: Vec<bool> = self.chunks.values().map(|chunk| chunk.active).collect();
                graph.evaluate(&active)
            }
        };
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Reject definitions without identity or with non-finite or non-positive geometry and health.
fn validate_definition(definition: &DestructibleChunkDefinition) -> Result<(), DestructionError> {
    if definition.id.is_empty() || definition.material.is_empty() {
        return Err(DestructionError::InvalidChunkIdentity);
    }
    let finite_center = definition.center.is_finite();
    let positive_size = definition.size.is_finite() && definition.size.cmpgt(DVec3::ZERO).all();
    let positive_health = definition.hit_points.is_finite() && definition.hit_points > 0.0;
    if !finite_center || !positive_size || !positive_health {
        return Err(DestructionError::InvalidChunkDimensions);
    }
    Ok(())
}

/// Whether a sphere touches a chunk's box (closest-point distance, inclusive).
fn sphere_intersects_box(center: DVec3, radius: f64, chunk: &DestructibleChunkDefinition) -> bool {
    let half = chunk.size / 2.0;
    // Chunk sizes are validated positive, so the box's lower corner never exceeds its upper corner.
    let closest = center.min(chunk.center + half).max(chunk.center - half);
    (center - closest).length_squared() <= radius * radius
}

/// Slab test: the entry parameter of a ray into a chunk's box, or `None` when it misses.
fn ray_box_distance(origin: DVec3, direction: DVec3, chunk: &DestructibleChunkDefinition) -> Option<f64> {
    let mut minimum: f64 = 0.0;
    let mut maximum = f64::INFINITY;
    for axis in 0..3 {
        let half = chunk.size[axis] / 2.0;
        let lower = chunk.center[axis] - half;
        let upper = chunk.center[axis] + half;
        if direction[axis].abs() < PARALLEL_EPSILON {
            if origin[axis] < lower || origin[axis] > upper {
                return None;
            }
            continue;
        }
        let first = (lower - origin[axis]) / direction[axis];
        let second = (upper - origin[axis]) / direction[axis];
        minimum = minimum.max(first.min(second));
        maximum = maximum.min(first.max(second));
        if maximum < minimum {
            return None;
        }
    }
    Some(minimum)
}

/// Strict overlap of two axis-aligned boxes on every axis.
fn boxes_overlap(first_center: DVec3, first_half: DVec3, second_center: DVec3, second_half: DVec3) -> bool {
    (first_center - second_center).abs().cmplt(first_half + second_half).all()
}
