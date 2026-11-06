// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Destructible chunk structures: queued radial damage committed at simulation boundaries, optional ground-support
//! collapse (against a flat ground or a sampled surface such as cratered terrain), optional load-path stress with
//! staged, delayed overload failure, static ray and occupancy queries, and renderer-free box geometry.
//!
//! A typical frame queues hits with [`ChunkStructure::queue_damage`], counts down overloaded chunks with
//! [`ChunkStructure::advance_stress`], calls [`ChunkStructure::commit`] once at the simulation boundary, turns the
//! [`ChunkChangeSet`] into collision changes ([`create_removal_mutation_batch`](crate::create_removal_mutation_batch))
//! and debris, and rebuilds the visible mesh from [`ChunkStructure::active_chunks`] with [`create_chunk_mesh_data`].
//! [`ChunkStructure::stress_state`] telegraphs strained and failing chunks before they fall.

mod chunk_bodies;
mod chunk_fracture;
mod chunk_geometry;
mod chunk_shape;
mod chunk_stress;
mod chunk_structure;
mod chunk_support;

pub use chunk_bodies::{
    DebrisOptions,
    DebrisPiece,
    DebrisPool,
    DebrisRelease,
    DebrisSettlement,
    StructureColliderOptions,
    StructureColliders,
};
pub use chunk_fracture::{
    FRACTURE_ID_SEPARATOR,
    fracture_chunk,
    fracture_sites,
    voronoi_fracture,
};
pub use chunk_shape::ChunkPolyhedron;

pub use chunk_geometry::{
    FRAME_MATERIAL,
    INTERIOR_MATERIAL,
    create_chunk_mesh_data,
};
pub use chunk_stress::{
    ChunkMaterialStrength,
    ChunkStressOptions,
    ChunkStressStage,
    ChunkStressState,
    DEFAULT_WARNING_RATIO,
    OVERLOAD_RATIO,
};
pub use chunk_structure::{
    ChunkChangeSet,
    ChunkDamageState,
    ChunkRaycastHit,
    ChunkStructure,
    DestructibleChunkDefinition,
    RadialDamage,
};
pub use chunk_support::{
    ChunkSupportOptions,
    ChunkSupportState,
    DEFAULT_CONTACT_TOLERANCE,
};

/// Invalid chunk definitions, damage, queries, or support options. Messages are stable diagnostics.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DestructionError {
    /// A chunk had an empty id or material.
    #[error("Chunks need an id, material, center, and size.")]
    InvalidChunkIdentity,
    /// A chunk had a non-finite center, or non-positive (or non-finite) dimensions or hit points.
    #[error("Chunk coordinates must be finite; dimensions and hit points must be positive.")]
    InvalidChunkDimensions,
    /// Two chunks shared an id.
    #[error("Duplicate destructible chunk id: {0}")]
    DuplicateChunk(String),
    /// A damage center had a non-finite coordinate.
    #[error("Damage center needs three finite coordinates.")]
    InvalidDamageCenter,
    /// A damage radius or amount was not finite and positive.
    #[error("Damage radius and amount must be finite positive values.")]
    InvalidDamageAmount,
    /// A raycast had non-finite vectors or a maximum distance that was not finite and positive.
    #[error("Raycast needs three-component vectors and a positive maximum distance.")]
    InvalidRaycast,
    /// An occupancy query had a non-finite center or non-positive half extents.
    #[error("Occupancy queries need a finite center and positive three-component half extents.")]
    InvalidOccupancyQuery,
    /// Support options had a non-finite ground height or a negative or non-finite span or tolerance.
    #[error("Chunk support requires a finite ground height and nonnegative span and contact tolerance.")]
    InvalidSupportOptions,
    /// Stress or a ground surface was requested for a structure built without support options.
    #[error("Chunk stress and ground surfaces require support options.")]
    SupportDisabled,
    /// Stress options had a non-positive density or strength, an empty or duplicate material, a lateral fraction
    /// outside `(0, 1]`, a warning ratio outside `(0, 1)`, or a negative failure delay.
    #[error("Chunk stress requires positive densities and strengths, unique materials, and valid ratios and delays.")]
    InvalidStressOptions,
    /// An elapsed time was negative or non-finite.
    #[error("Elapsed seconds must be finite and nonnegative.")]
    InvalidElapsedTime,
    /// A chunk shape was not a closed convex polyhedron with finite vertices, or its bounds did not match the chunk's
    /// center and size.
    #[error("Chunk shapes must be closed convex polyhedra whose bounds match the chunk's center and size.")]
    InvalidChunkShape,
}
