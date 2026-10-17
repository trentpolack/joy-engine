// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rigid bodies, raycast vehicles, and scene queries behind a backend-neutral API.
//!
//! [`PhysicsWorld`] owns a rapier3d solver and every body and vehicle created through it, and hands out copyable
//! [`BodyHandle`] and [`VehicleHandle`] values. Handles carry the identity of the world that created them, so a handle
//! from another world (or a removed body) is reported as unknown instead of aliasing an unrelated body.
//!
//! Coordinates are meters in Y-up world space; rotations are XYZ Euler radians applied X, then Y, then Z. Vehicles use
//! up = +Y, forward = +Z, and right = +X in chassis space. Time is in seconds and the solver advances in fixed steps
//! with bounded catch-up.
//!
//! Authored physics values preserve the established tuning contract. Rapier parameter mapping and solver limitations
//! are documented on the individual options and in `docs/rust-simulation.md`.
//!
//! [`destruction`] holds solver-independent destructible chunk structures, and [`PhysicsBackend`] is the
//! shape-residency contract they synchronize through; `PhysicsWorld` implements it as the CPU backend.
//!
//! [`SdfShape`] colliders describe solids by signed distance (analytical primitives or meshes baked into voxel grids)
//! and collide through optimization-based contact search; see [`PhysicsWorld::create_sdf_body`].

mod backend_contract;
mod convert;
pub mod destruction;
mod physics_body;
mod physics_vehicle;
mod physics_world;
mod sdf;

pub use backend_contract::{
    PhysicsBackend, PhysicsCollisionShape, PhysicsMutationBatch, PhysicsQueryHit, PhysicsResidency,
    create_removal_mutation_batch,
};
pub use destruction::{
    ChunkChangeSet, ChunkDamageState, ChunkRaycastHit, ChunkStructure, ChunkSupportOptions, ChunkSupportState,
    DestructibleChunkDefinition, DestructionError, RadialDamage, create_chunk_mesh_data,
};
pub use physics_body::{BODY_DEFAULTS, BodyOptions};
pub use physics_vehicle::{
    VEHICLE_DEFAULTS, VehicleInput, VehicleOptions, VehicleTuning, WHEEL_DEFAULTS, WheelOptions,
};
pub use physics_world::{PhysicsWorld, PhysicsWorldOptions};
pub use sdf::{SDF_BAKE_DEFAULTS, SdfBakeOptions, SdfCollisionStatistics, SdfPrimitive, SdfShape};

use glam::Vec3;

use crate::core::format::format_bound;

/// Opaque handle to a rigid body owned by one [`PhysicsWorld`].
///
/// Handles are plain values: copying one never extends the body's lifetime. Operations on a removed body, or on a
/// handle from another world, fail with [`PhysicsError::UnknownBody`] or return `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyHandle {
    pub(crate) world: u64,
    pub(crate) raw: rapier3d::dynamics::RigidBodyHandle,
}

/// Opaque handle to a raycast vehicle owned by one [`PhysicsWorld`]. The vehicle borrows its chassis body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VehicleHandle {
    pub(crate) world: u64,
    pub(crate) id: u64,
}

/// Copied world-space pose of a body or wheel. Mutating it never changes the simulation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsTransform {
    /// World position in meters.
    pub position: Vec3,
    /// XYZ Euler radians applied X, then Y, then Z (`crate::core::euler_from_quat`).
    pub rotation: Vec3,
}

/// Closest hit along a world-space segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit {
    /// The body that was hit (a copied handle).
    pub body: BodyHandle,
    /// World-space hit point in meters.
    pub point: Vec3,
    /// World-space unit surface normal at the hit point.
    pub normal: Vec3,
    /// Distance from the segment start in meters.
    pub distance: f32,
}

/// Physics validation and ownership failures. Messages follow the TypeScript engine.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PhysicsError {
    /// A scalar was non-finite or outside its inclusive range.
    #[error("{name} must be finite and between {} and {}.", format_bound(*.minimum), format_bound(*.maximum))]
    OutOfRange {
        /// Option or argument name, in TypeScript spelling.
        name: &'static str,
        /// Inclusive lower bound.
        minimum: f64,
        /// Inclusive upper bound.
        maximum: f64,
    },
    /// A vector had a non-finite component.
    #[error("{0} must contain three finite numbers.")]
    NonFiniteVector(&'static str),
    /// A box collider had a zero or negative half extent.
    #[error("Box halfExtents must be positive.")]
    NonPositiveHalfExtents,
    /// An infinite plane was given a nonzero mass.
    #[error("Infinite planes must be static (mass 0).")]
    DynamicPlane,
    /// The body handle does not name a live body in this world.
    #[error("Physics body is not owned by this world or has been removed.")]
    UnknownBody,
    /// The vehicle handle does not name a live vehicle in this world.
    #[error("Physics vehicle is not owned by this world or has been removed.")]
    UnknownVehicle,
    /// The chassis was static, foreign, removed, or already carried a vehicle.
    #[error("Vehicle chassis must be an unattached dynamic body owned by this world.")]
    InvalidChassis,
    /// A vehicle was configured without wheels.
    #[error("A vehicle requires at least one wheel.")]
    NoWheels,
    /// A mutation batch named an empty shape id.
    #[error("Physics mutation batches need a nonnegative sequence and nonempty shape ids.")]
    InvalidMutationBatch,
    /// A mutation batch was older than the newest batch already applied.
    #[error("Physics mutation batch {sequence} is older than the applied batch {last}.")]
    StaleMutationBatch {
        /// The rejected batch's sequence.
        sequence: u64,
        /// The newest applied sequence.
        last: u64,
    },
    /// A batch added a shape id that was already resident (or added it twice).
    #[error("Physics shape {0} is already resident.")]
    DuplicateShape(String),
    /// A batch added a shape whose center was not finite or whose half extents were not finite and positive.
    #[error("Physics shape {0} needs a finite center and positive half extents.")]
    InvalidShape(String),
    /// An SDF primitive or mesh was rejected; the message names the problem.
    #[error("Invalid SDF shape: {0}.")]
    InvalidSdfShape(&'static str),
}
