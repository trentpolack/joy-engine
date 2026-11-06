// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Destructible structures in a physics world: static colliders for the chunks still standing and a pool of rigid
//! debris for the chunks that leave, both in the same world as everything else, so falling pieces land on what survives,
//! block bodies and queries, and stay solid where they come to rest.
//!
//! After every [`ChunkStructure::commit`](super::ChunkStructure::commit) that changed something, call
//! [`StructureColliders::sync`] with the structure's active chunks first (removing a collider wakes whatever rested on
//! it) and then [`DebrisPool::release`] for the chunks the change set lists. Each
//! frame or tick, after stepping physics, [`DebrisPool::settle`] keeps the pool within its budget: the oldest sleeping
//! pieces freeze into static rubble once too many are moving, pieces still moving after
//! [`DebrisOptions::maximum_moving_seconds`] freeze where they are, and the oldest pieces leave once the pool is full.
//!
//! Debris shapes are the chunks' own: a convex hull of a fractured piece's polyhedron, or its box, shrunk slightly by
//! [`DebrisOptions::release_scale`] so a piece released from a tightly fitted wall does not start wedged against its
//! neighbors. Units are meters, kilograms, and seconds.

use indexmap::IndexMap;
use joy_core::PrimitiveShape;
use joy_core::math::{
    DVec3,
    Vec3,
};

use super::DestructibleChunkDefinition;
use super::chunk_shape::ChunkPolyhedron;
use crate::{
    BodyHandle,
    BodyOptions,
    CollisionGroups,
    PhysicsError,
    PhysicsWorld,
};

// ---------------------------------------------------------------------------------------------------------------------
// Structure Colliders
// ---------------------------------------------------------------------------------------------------------------------

/// Material and layers of the static colliders standing in for a structure's active chunks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructureColliderOptions {
    /// Collision layers of every chunk collider.
    pub groups: CollisionGroups,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution in `[0, 1]`.
    pub restitution: f32,
}

impl Default for StructureColliderOptions {
    fn default() -> Self {
        Self {
            groups: CollisionGroups::ALL,
            friction: 0.8,
            restitution: 0.0,
        }
    }
}

/// One static body per active chunk of a [`ChunkStructure`], keyed by chunk id, in a world the caller owns.
#[derive(Debug, Default)]
pub struct StructureColliders {
    options: StructureColliderOptions,
    bodies: IndexMap<String, BodyHandle>,
}

impl StructureColliders {
    /// Colliders created with `options`; nothing exists until the first [`sync`](Self::sync).
    pub fn new(options: StructureColliderOptions) -> Self {
        Self {
            options,
            bodies: IndexMap::new(),
        }
    }

    /// Match colliders to `chunks` (usually `structure.active_chunks()`), in their order: chunks no longer listed lose
    /// their collider (waking bodies resting on it) and newly listed chunks gain one. Fails if a chunk's shape cannot
    /// become a collider; colliders already matched stay.
    pub fn sync<'a>(
        &mut self,
        world: &mut PhysicsWorld,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
    ) -> Result<(), PhysicsError> {
        let chunks: Vec<&DestructibleChunkDefinition> = chunks.into_iter().collect();
        let listed: std::collections::HashSet<&str> = chunks.iter().map(|chunk| chunk.id.as_str()).collect();
        self.bodies.retain(|id, body| {
            let keep = listed.contains(id.as_str()) && world.contains_body(*body);
            if !keep {
                world.remove_body(*body);
            }
            keep
        });
        for chunk in chunks {
            if self.bodies.contains_key(&chunk.id) {
                continue;
            }
            let options = BodyOptions {
                mass: 0.0,
                friction: self.options.friction,
                restitution: self.options.restitution,
                groups: self.options.groups,
                ..BodyOptions::new(PrimitiveShape::Box)
            };
            let body = create_chunk_body(world, chunk, 1.0, options)?;
            self.bodies.insert(chunk.id.clone(), body);
        }
        Ok(())
    }

    /// The collider standing in for an active chunk.
    pub fn body(&self, chunk_id: &str) -> Option<BodyHandle> {
        self.bodies.get(chunk_id).copied()
    }

    /// The chunk a collider stands in for, for mapping contacts and query hits back to the structure.
    pub fn chunk_for_body(&self, body: BodyHandle) -> Option<&str> {
        self.bodies.iter().find(|(_, owned)| **owned == body).map(|(id, _)| id.as_str())
    }

    /// Number of chunk colliders.
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Whether no chunk has a collider.
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// Remove every collider from `world`.
    pub fn clear(&mut self, world: &mut PhysicsWorld) {
        for (_, body) in self.bodies.drain(..) {
            world.remove_body(body);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Debris
// ---------------------------------------------------------------------------------------------------------------------

/// How released chunks behave as rigid debris, and how many the pool keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebrisOptions {
    /// Collision layers of debris bodies.
    pub groups: CollisionGroups,
    /// Debris density in kilograms per cubic meter, used when a release gives no mass.
    pub density: f32,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution in `[0, 1]`.
    pub restitution: f32,
    /// Per-second linear velocity loss fraction in `[0, 1]`.
    pub linear_damping: f32,
    /// Per-second angular velocity loss fraction in `[0, 1]`.
    pub angular_damping: f32,
    /// Size of a released piece relative to its chunk, in `(0, 1]`, so it does not start wedged between neighbors.
    pub release_scale: f64,
    /// Pieces allowed to keep simulating; beyond it the oldest sleeping pieces freeze into static rubble.
    pub maximum_moving: usize,
    /// Pieces kept at all, moving or frozen; beyond it the oldest leave the world.
    pub maximum_pieces: usize,
    /// Seconds a piece may keep moving before it freezes where it is, so a piece that never comes to rest (jittering
    /// in a crevice) stops costing solver time.
    pub maximum_moving_seconds: f64,
}

impl Default for DebrisOptions {
    fn default() -> Self {
        Self {
            groups: CollisionGroups::ALL,
            density: 2000.0,
            friction: 0.8,
            restitution: 0.05,
            linear_damping: 0.05,
            angular_damping: 0.2,
            release_scale: 0.97,
            maximum_moving: 96,
            maximum_pieces: 512,
            maximum_moving_seconds: 12.0,
        }
    }
}

/// How one chunk leaves: its starting velocity, and optional mass and size overrides.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DebrisRelease {
    /// Starting linear velocity in meters per second.
    pub velocity: Vec3,
    /// Mass in kilograms; `None` uses [`DebrisOptions::density`] times the piece's volume. Pass
    /// `ChunkStressOptions::chunk_mass` so debris weighs what the structure carried.
    pub mass: Option<f32>,
    /// Size relative to the chunk in `(0, 1]`; `None` uses [`DebrisOptions::release_scale`].
    pub scale: Option<f64>,
    /// Starting XYZ Euler rotation in radians about the piece's center of volume.
    pub rotation: Vec3,
}

/// One released chunk: its body and its shape in the body's frame (meters, centered on the body origin).
#[derive(Clone, Debug, PartialEq)]
pub struct DebrisPiece {
    /// The released chunk's id.
    pub chunk_id: String,
    /// The chunk's material name.
    pub material: String,
    /// The dynamic (or, once settled, static) body.
    pub body: BodyHandle,
    /// The piece's solid in its body's frame (centered on its center of volume), already shrunk by the release scale;
    /// draw it at the body's pose.
    pub local_shape: ChunkPolyhedron,
    /// Whether the piece froze into static rubble.
    pub settled: bool,
    /// Simulation seconds since release.
    pub age_seconds: f64,
}

impl DebrisPiece {
    /// The piece as a chunk definition in its body's frame (centered on the origin), for building its mesh with
    /// [`create_chunk_mesh_data`](super::create_chunk_mesh_data) and drawing it at the body's pose.
    pub fn local_chunk(&self) -> DestructibleChunkDefinition {
        let (minimum, maximum) = self.local_shape.bounds();
        DestructibleChunkDefinition {
            id: self.chunk_id.clone(),
            center: (minimum + maximum) / 2.0,
            size: maximum - minimum,
            material: self.material.clone(),
            hit_points: 1.0,
            shape: Some(self.local_shape.clone()),
        }
    }
}

/// Changes [`DebrisPool::settle`] made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebrisSettlement {
    /// Chunk ids of pieces that froze into static rubble.
    pub settled: Vec<String>,
    /// Chunk ids of pieces removed to stay within the pool size.
    pub removed: Vec<String>,
}

/// Released chunks as rigid bodies in a world the caller owns, oldest first, at most one per chunk id.
#[derive(Debug, Default)]
pub struct DebrisPool {
    options: DebrisOptions,
    pieces: IndexMap<String, DebrisPiece>,
}

impl DebrisPool {
    /// An empty pool releasing pieces with `options`.
    pub fn new(options: DebrisOptions) -> Self {
        Self {
            options,
            pieces: IndexMap::new(),
        }
    }

    /// Release a chunk as a dynamic body with its own shape, as `release` describes. Returns the body (apply spin or
    /// impulses to it as needed), or `None` for a chunk already released. Fails without side effects when the shape
    /// cannot become a collider.
    pub fn release(
        &mut self,
        world: &mut PhysicsWorld,
        chunk: &DestructibleChunkDefinition,
        release: DebrisRelease,
    ) -> Result<Option<BodyHandle>, PhysicsError> {
        if self.pieces.contains_key(&chunk.id) {
            return Ok(None);
        }
        let scale = release.scale.unwrap_or(self.options.release_scale).clamp(f64::EPSILON, 1.0);
        let volume = (chunk.volume() * (scale * scale * scale)) as f32;
        let mass = release.mass.unwrap_or(self.options.density * volume).max(f32::EPSILON);
        let options = BodyOptions {
            mass,
            friction: self.options.friction,
            restitution: self.options.restitution,
            linear_damping: self.options.linear_damping,
            angular_damping: self.options.angular_damping,
            groups: self.options.groups,
            ..BodyOptions::new(PrimitiveShape::Box)
        };
        let body = create_chunk_body(world, chunk, scale, options)?;
        world.set_body_velocity(body, release.velocity)?;
        if release.rotation != Vec3::ZERO {
            world.set_body_transform(body, None, Some(release.rotation))?;
        }
        let local_shape = chunk.solid().translated(-body_origin(chunk)).scaled_about(DVec3::ZERO, scale);
        self.pieces.insert(
            chunk.id.clone(),
            DebrisPiece {
                chunk_id: chunk.id.clone(),
                material: chunk.material.clone(),
                body,
                local_shape,
                settled: false,
                age_seconds: 0.0,
            },
        );
        Ok(Some(body))
    }

    /// Keep the pool within its budget after a physics step of `elapsed_seconds`: freeze pieces moving longer than
    /// `maximum_moving_seconds`, then the oldest sleeping pieces while more than `maximum_moving` still simulate, then
    /// remove the oldest pieces while more than `maximum_pieces` remain. Pieces whose body disappeared are forgotten.
    pub fn settle(&mut self, world: &mut PhysicsWorld, elapsed_seconds: f64) -> DebrisSettlement {
        let mut report = DebrisSettlement::default();
        self.pieces.retain(|_, piece| world.contains_body(piece.body));
        let elapsed = if elapsed_seconds.is_finite() { elapsed_seconds.max(0.0) } else { 0.0 };

        let mut moving = 0;
        for piece in self.pieces.values_mut().filter(|piece| !piece.settled) {
            piece.age_seconds += elapsed;
            let overdue = piece.age_seconds > self.options.maximum_moving_seconds;
            if overdue && world.freeze_body(piece.body).is_ok() {
                piece.settled = true;
                report.settled.push(piece.chunk_id.clone());
                continue;
            }
            moving += 1;
        }
        for piece in self.pieces.values_mut() {
            if moving <= self.options.maximum_moving {
                break;
            }
            if piece.settled || world.is_sleeping(piece.body) != Some(true) {
                continue;
            }
            if world.freeze_body(piece.body).is_ok() {
                piece.settled = true;
                moving -= 1;
                report.settled.push(piece.chunk_id.clone());
            }
        }

        while self.pieces.len() > self.options.maximum_pieces {
            let Some((id, piece)) = self.pieces.shift_remove_index(0) else {
                break;
            };
            world.remove_body(piece.body);
            report.removed.push(id);
        }
        report
    }

    /// Pieces still simulating (not frozen into rubble).
    pub fn moving(&self) -> usize {
        self.pieces.values().filter(|piece| !piece.settled).count()
    }

    /// Every piece, oldest first.
    pub fn pieces(&self) -> impl Iterator<Item = &DebrisPiece> {
        self.pieces.values()
    }

    /// The piece released for a chunk.
    pub fn piece(&self, chunk_id: &str) -> Option<&DebrisPiece> {
        self.pieces.get(chunk_id)
    }

    /// Number of pieces.
    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    /// Whether the pool holds no pieces.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Remove every piece from `world`.
    pub fn clear(&mut self, world: &mut PhysicsWorld) {
        for (_, piece) in self.pieces.drain(..) {
            world.remove_body(piece.body);
        }
    }
}

/// Where a chunk's body frame sits: its shape's center of volume, or its box's center, so a body's position is its
/// center of mass whichever way the piece comes to rest.
pub(crate) fn body_origin(chunk: &DestructibleChunkDefinition) -> DVec3 {
    match &chunk.shape {
        Some(shape) => shape.centroid(),
        None => chunk.center,
    }
}

/// A body at the chunk's [`body_origin`] with the chunk's shape (its hull, or its box) scaled by `scale` about it.
fn create_chunk_body(
    world: &mut PhysicsWorld,
    chunk: &DestructibleChunkDefinition,
    scale: f64,
    options: BodyOptions,
) -> Result<BodyHandle, PhysicsError> {
    let origin = body_origin(chunk);
    let position = origin.as_vec3();
    match &chunk.shape {
        Some(shape) => {
            let points: Vec<Vec3> =
                shape.vertices.iter().map(|vertex| ((*vertex - origin) * scale).as_vec3()).collect();
            world.create_convex_body(
                &points,
                BodyOptions {
                    position,
                    ..options
                },
            )
        }
        None => world.create_body(BodyOptions {
            position,
            half_extents: (chunk.size * (scale / 2.0)).as_vec3(),
            ..options
        }),
    }
}
