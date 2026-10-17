// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Signed distance field (SDF) colliders with optimization-based contact generation.
//!
//! An [`SdfShape`] describes a solid by its signed distance function: analytical primitives, or a voxel grid baked from
//! any triangle mesh (FORM output included) without convex decomposition. Bodies built with
//! [`PhysicsWorld::create_sdf_body`](super::PhysicsWorld::create_sdf_body) collide with each other and with the
//! world's boxes, spheres, and planes; everything else about them (solver, materials, damping, raycasts) works like
//! any other body.
//!
//! Contacts follow Giles and Andrews, *Real-Time Collision Handling for Signed Distance Fields via Caching and
//! Importance Sampling* (Pacific Graphics 2026), adapted from their MIT-licensed reference implementation
//! (<https://github.com/savant117/sdf-dcd-demo>, Copyright (c) 2026 Chris Giles; see `LICENSE-sdf-dcd-demo`). For each
//! pair with overlapping bounds, a few seed points are optimized toward the deepest overlap with Nelder-Mead: last
//! step's points (a temporal cache that also warm starts the solver) and a random high-curvature seed. The resulting
//! sparse manifold goes to rapier's solver.
//!
//! This is a prototype. Differences from the paper:
//! - Contact normals and depths are read one-sidedly at the optimized point's projections onto both surfaces (the
//!   deeper reading wins) instead of from the deeper field at the point itself (Eq. 5), which tilts resting contacts
//!   wherever an edge or corner meets a face.
//! - Cached points are stored in both bodies' frames and carried by their poses rather than advected by velocity
//!   (Eq. 6). They refine to a tighter tolerance and keep their carried position unless the refinement is deeper, so
//!   points on flat faces stop trading places between steps.
//! - Pairs involving SDF colliders opt out of rapier's contact recycling, which would freeze the cache's step-by-step
//!   refinement while bodies settle.
//! - Contact sampling uses fixed per-pair seeded streams, so identical worlds produce identical results on every
//!   target; the solver's scalar math keeps native and WebAssembly builds in agreement.
//!
//! Limits: collision remains discrete, so fast or thin bodies can tunnel. SDF bodies ignore meshes, compounds, sensors,
//! shape casts, and continuous collision detection, and only the Nelder-Mead optimizer is implemented. Manifold points
//! spread across small curved-on-flat overlaps, which acts like rolling resistance on rolling SDF bodies.

mod sdf_collider;
mod sdf_contact;
mod sdf_dispatcher;
mod sdf_primitives;
mod sdf_shape;
mod sdf_voxel;

pub use sdf_shape::{SDF_BAKE_DEFAULTS, SdfBakeOptions, SdfPrimitive, SdfShape};

pub(crate) use sdf_collider::SdfCollider;
pub(crate) use sdf_dispatcher::{SdfCollisionCounters, SdfQueryDispatcher};

/// SDF contact search work during the most recent fixed step, for diagnostics. Counts never affect simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SdfCollisionStatistics {
    /// Collider pairs involving an SDF whose bounds overlapped.
    pub pairs: u32,
    /// Manifold points within the solver's contact distance.
    pub contacts: u32,
    /// Nelder-Mead iterations across all pairs.
    pub iterations: u32,
    /// Distance field evaluations across all pairs (two per objective evaluation).
    pub distance_queries: u32,
}
