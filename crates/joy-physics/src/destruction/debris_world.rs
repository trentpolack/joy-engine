// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A complete debris simulation for destructible structures: one physics world holding static colliders for every
//! structure's standing chunks, a budgeted [`DebrisPool`] of rigid pieces, and per-piece strike tracking so moving
//! debris can hurt what it lands on.
//!
//! Games and examples used to assemble this by hand around the pool. A typical step after a structure commit:
//!
//! 1. [`DebrisWorld::sync_structure`] for every structure that changed, so pieces land on what survives and removed
//!    colliders never overlap their own debris.
//! 2. [`DebrisWorld::release`] for each piece the change set produced (the caller decides sizes, launch velocities,
//!    and how pieces are drawn), then any spin with [`DebrisWorld::apply_impulse`].
//! 3. [`DebrisWorld::step`] once per frame or simulation tick, which also keeps the pool within its budget.
//! 4. [`DebrisWorld::strikes`] against the step's strikeable targets, when debris should deal damage.
//!
//! The ground is the caller's: add boxes or heightfields through [`DebrisWorld::simulation_mut`]. Units are meters,
//! seconds, kilograms, and joules; positions are Y-up world space. The world steps with a fixed step, so feeding it from
//! a seeded simulation's tick replays identically on native and WebAssembly builds.

use std::collections::BTreeMap;

use joy_core::math::{
    DVec3,
    Vec3,
    deterministic,
};

use super::{
    DebrisOptions,
    DebrisPiece,
    DebrisPool,
    DebrisRelease,
    DestructibleChunkDefinition,
    StructureColliderOptions,
    StructureColliders,
};
use crate::{
    BodyHandle,
    PhysicsError,
    PhysicsSimulation,
    PhysicsSimulationOptions,
    PhysicsTransform,
};

/// How a debris world simulates and budgets its pieces.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DebrisWorldOptions {
    /// Gravity, fixed step, and catch-up limit of the debris simulation.
    pub simulation: PhysicsSimulationOptions,
    /// Rigid piece material and budgets.
    pub pool: DebrisOptions,
    /// Material of the colliders standing in for structures' active chunks.
    pub structure: StructureColliderOptions,
}

/// Makes a released piece able to strike targets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebrisStriker {
    /// Mass used for strike energy, in kilograms (independent of the body's simulated mass, so strike tuning never
    /// changes how pieces tumble).
    pub kilograms: f64,
    /// Target ids this piece never strikes, for example units a falling chunk already crushed.
    pub exempt: Vec<u64>,
}

/// A target debris can strike: an upright cylinder standing on `base`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebrisTarget {
    /// The caller's stable id; each piece strikes each id at most once.
    pub id: u64,
    /// Whether the target can be struck this step (alive, mobile, and so on).
    pub strikeable: bool,
    /// Center of the cylinder's base, in meters.
    pub base: DVec3,
    /// Cylinder radius in meters.
    pub radius: f64,
    /// Cylinder height in meters.
    pub height: f64,
}

/// A moving piece reached a target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebrisStrike {
    /// Index of the struck target in the slice passed to [`DebrisWorld::strikes`].
    pub target: usize,
    /// The piece's center at the strike, in meters.
    pub point: DVec3,
    /// The piece's velocity at the strike, in meters per second.
    pub velocity: DVec3,
    /// The piece's kinetic energy relative to the ground, in joules.
    pub joules: f64,
}

/// What one [`DebrisWorld::step`] changed.
#[derive(Debug, Default)]
pub struct DebrisStep {
    /// Ids of pieces removed to stay within the pool size; presentation should drop their meshes.
    pub removed: Vec<String>,
    /// Ids of pieces that froze into static rubble this step.
    pub settled: Vec<String>,
    /// A physics step failure, if any; the pool still settled so its budget holds.
    pub failure: Option<PhysicsError>,
}

/// Live debris counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebrisCounts {
    /// Pieces still simulating.
    pub moving: usize,
    /// Pieces frozen into static rubble.
    pub rubble: usize,
}

/// What a released striker can still hit.
#[derive(Debug)]
struct StrikeRecord {
    // Half the piece's box diagonal: its reach around its center, in meters.
    reach: f64,
    kilograms: f64,
    spent_on: Vec<u64>,
}

/// Owns the debris physics world, every structure's colliders, the rigid pool, and strike records.
pub struct DebrisWorld {
    world: PhysicsSimulation,
    structure_options: StructureColliderOptions,
    // One collider set per structure slot, matched to that structure's active chunks.
    structures: Vec<StructureColliders>,
    pool: DebrisPool,
    strikers: BTreeMap<String, StrikeRecord>,
}

impl DebrisWorld {
    /// An empty debris world with no ground; add one through [`simulation_mut`](Self::simulation_mut).
    pub fn new(options: DebrisWorldOptions) -> Result<Self, PhysicsError> {
        Ok(Self {
            world: PhysicsSimulation::new(options.simulation)?,
            structure_options: options.structure,
            structures: Vec::new(),
            pool: DebrisPool::new(options.pool),
            strikers: BTreeMap::new(),
        })
    }

    /// The debris simulation, for poses and queries.
    pub fn simulation(&self) -> &PhysicsSimulation {
        &self.world
    }

    /// The debris simulation, for adding ground and other static colliders.
    pub fn simulation_mut(&mut self) -> &mut PhysicsSimulation {
        &mut self.world
    }

    /// Match structure `slot`'s colliders to `chunks` (its active chunks). Slots are created on first use; call this
    /// before releasing a structure's pieces so they never start inside their own former colliders.
    pub fn sync_structure<'a>(
        &mut self,
        slot: usize,
        chunks: impl IntoIterator<Item = &'a DestructibleChunkDefinition>,
    ) -> Result<(), PhysicsError> {
        while self.structures.len() <= slot {
            self.structures.push(StructureColliders::new(self.structure_options));
        }
        self.structures[slot].sync(&mut self.world, chunks)
    }

    /// Number of structure slots with colliders.
    pub fn structure_slots(&self) -> usize {
        self.structures.len()
    }

    /// Colliders standing in for every structure's active chunks.
    pub fn structure_collider_count(&self) -> usize {
        self.structures.iter().map(StructureColliders::len).sum()
    }

    /// Release `piece` as rigid debris (see [`DebrisPool::release`]); with a `striker`, it can strike targets while it
    /// moves. Returns the body, or `None` for an id already released.
    pub fn release(
        &mut self,
        piece: &DestructibleChunkDefinition,
        release: DebrisRelease,
        striker: Option<DebrisStriker>,
    ) -> Result<Option<BodyHandle>, PhysicsError> {
        let pool_scale = self.pool_scale(release);
        let Some(body) = self.pool.release(&mut self.world, piece, release)? else {
            return Ok(None);
        };
        if let Some(striker) = striker {
            self.strikers.insert(
                piece.id.clone(),
                StrikeRecord {
                    reach: (piece.size * pool_scale).length() / 2.0,
                    kilograms: striker.kilograms,
                    spent_on: striker.exempt,
                },
            );
        }
        Ok(Some(body))
    }

    /// Apply an impulse (newton-seconds) to a piece's body, at `point` in world space or its center of mass.
    pub fn apply_impulse(&mut self, body: BodyHandle, impulse: Vec3, point: Option<Vec3>) -> Result<(), PhysicsError> {
        self.world.apply_impulse(body, impulse, point)
    }

    /// Advance the debris simulation by `elapsed_seconds` and keep the pool within its budget.
    pub fn step(&mut self, elapsed_seconds: f64) -> DebrisStep {
        let failure = self.world.step(elapsed_seconds as f32).err();
        let settlement = self.pool.settle(&mut self.world, elapsed_seconds);
        for id in &settlement.removed {
            self.strikers.remove(id);
        }
        DebrisStep {
            removed: settlement.removed,
            settled: settlement.settled,
            failure,
        }
    }

    /// Strikes by moving pieces at least `minimum_speed` (meters per second) fast that reach a strikeable target's
    /// cylinder, in piece then target order. Each piece strikes each target id at most once over its life.
    pub fn strikes(&mut self, targets: &[DebrisTarget], minimum_speed: f64) -> Vec<DebrisStrike> {
        let mut strikes = Vec::new();
        for piece in self.pool.pieces().filter(|piece| !piece.settled) {
            let Some(record) = self.strikers.get_mut(&piece.chunk_id) else {
                continue;
            };
            let (Some(pose), Some(velocity)) =
                (self.world.body_transform(piece.body), self.world.body_velocity(piece.body))
            else {
                continue;
            };
            let velocity = velocity.as_dvec3();
            let speed = velocity.length();
            if speed < minimum_speed {
                continue;
            }
            let center = pose.position.as_dvec3();
            for (index, target) in targets.iter().enumerate() {
                if !target.strikeable || record.spent_on.contains(&target.id) {
                    continue;
                }
                if !reaches(center, record.reach, target) {
                    continue;
                }
                record.spent_on.push(target.id);
                strikes.push(DebrisStrike {
                    target: index,
                    point: center,
                    velocity,
                    joules: 0.5 * record.kilograms * speed * speed,
                });
            }
        }
        strikes
    }

    /// A piece's pose at the latest fixed step.
    pub fn pose(&self, id: &str) -> Option<PhysicsTransform> {
        self.world.body_transform(self.pool.piece(id)?.body)
    }

    /// A piece's pose interpolated between fixed steps, for smooth drawing at any frame rate.
    pub fn interpolated_pose(&self, id: &str) -> Option<PhysicsTransform> {
        self.world.interpolated_body_transform(self.pool.piece(id)?.body)
    }

    /// The body velocity of a piece, in meters per second.
    pub fn velocity(&self, id: &str) -> Option<Vec3> {
        self.world.body_velocity(self.pool.piece(id)?.body)
    }

    /// Every piece, oldest first.
    pub fn pieces(&self) -> impl Iterator<Item = &DebrisPiece> {
        self.pool.pieces()
    }

    /// The piece released under `id`.
    pub fn piece(&self, id: &str) -> Option<&DebrisPiece> {
        self.pool.piece(id)
    }

    /// Pieces still simulating; compare with a rigid budget before releasing more.
    pub fn moving(&self) -> usize {
        self.pool.moving()
    }

    /// Number of pieces, moving or frozen.
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    /// Whether no pieces exist.
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }

    /// Moving and frozen counts.
    pub fn counts(&self) -> DebrisCounts {
        let moving = self.pool.moving();
        DebrisCounts {
            moving,
            rubble: self.pool.len() - moving,
        }
    }

    /// Remove every piece, keeping structure colliders and the ground.
    pub fn clear_pieces(&mut self) {
        self.pool.clear(&mut self.world);
        self.strikers.clear();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Implementation
    // -----------------------------------------------------------------------------------------------------------------

    /// The size scale the pool applies to a release.
    fn pool_scale(&self, release: DebrisRelease) -> f64 {
        release.scale.unwrap_or(self.pool.options().release_scale).clamp(f64::EPSILON, 1.0)
    }
}

/// Whether a piece reaching `reach` meters around `center` touches a target's upright cylinder.
fn reaches(center: DVec3, reach: f64, target: &DebrisTarget) -> bool {
    let horizontal = deterministic::hypot(center.x - target.base.x, center.z - target.base.z);
    let bottom = target.base.y - reach;
    let top = target.base.y + target.height + reach;
    horizontal < (target.radius + reach) && center.y >= bottom && center.y <= top
}

#[cfg(test)]
#[path = "debris_world.test.rs"]
mod tests;
