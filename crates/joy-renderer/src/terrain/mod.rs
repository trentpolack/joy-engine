// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Resident continuous-LOD (CLOD) terrain with shared native and browser rendering contracts.
//!
//! A [`TerrainAsset`] is a validated version 1 heightfield: a square power-of-two-plus-one grid of `u16` samples with
//! explicit world scales and a row-major list of fixed quadtree leaves, each with height bounds and monotone geometric
//! errors for every power-of-two stride. Rendering keeps the samples, one shared indexed patch lattice, patch records,
//! and indirect commands resident on the GPU; a frame updates only selection data and transforms.
//!
//! Selection ([`select_terrain`]) picks each leaf's fractional LOD from its errors and the conservative distance to its
//! bounds, limits projected cells to 32 CSS pixels, and lets shared edges adopt the coarser neighbor's fractional level.
//! Heights morph continuously toward the parent triangles before the index range changes, so neighbors of any
//! resolution meet without cracks or skirts. A target radius pins the finest detail around a protected point.
//! WebGPU-class backends select on the GPU with compute and indirect draws; WebGL 2 selects on the CPU.
//!
//! Terrain always draws as part of a scene: add a [`TerrainAsset`] to a `RenderScene` as a terrain instance, and the
//! renderer selects its LODs, draws it into the depth pre-pass and the lit scene pass (first among opaque geometry, as
//! the largest occluder), into every shadow view it touches, and into the scene-effect attachments. There is no separate
//! terrain draw path. Gameplay and collision use [`sample_terrain`], which always samples the finest authored surface,
//! never camera-selected topology.

mod terrain_asset;
mod terrain_geometry;
mod terrain_grid;
pub(crate) mod terrain_scene_pass;
mod terrain_selection;

pub use terrain_asset::{
    TerrainAsset,
    TerrainError,
    TerrainManifest,
    TerrainPatch,
    decode_terrain,
    load_terrain,
    parse_terrain,
    sample_height_triangle,
    sample_terrain,
};
pub use terrain_geometry::{
    TerrainDiagnostic,
    TerrainStats,
    TerrainTriangleCount,
};
pub use terrain_grid::{
    TerrainGrid,
    TerrainIndexRange,
    create_terrain_grid,
};
pub use terrain_selection::{
    TerrainSelection,
    TerrainView,
    reconcile_terrain_edges,
    sample_morphed_height,
    select_terrain,
    validate_terrain_view,
};

// WGSL sources. Terrain shaders are the shared geometry file followed by their own entry points.
const TERRAIN_GEOMETRY_SOURCE: &str = include_str!("../shaders/terrain-geometry.wgsl");
const TERRAIN_SELECT_SOURCE: &str = include_str!("../shaders/terrain-select.wgsl");
const TERRAIN_SCENE_SOURCE: &str = include_str!("../shaders/terrain-scene.wgsl");
const TERRAIN_CASTER_SOURCE: &str = include_str!("../shaders/terrain-caster.wgsl");
const TERRAIN_DEPTH_SOURCE: &str = include_str!("../shaders/terrain-depth.wgsl");

/// The shared geometry source with its bindings moved to `group`.
///
/// Each pipeline puts the terrain group right after its own groups, so no layout needs empty slots (which not every
/// browser's WebGPU accepts).
fn terrain_geometry_source(group: u32) -> String {
    TERRAIN_GEOMETRY_SOURCE.replace("@group(3)", &format!("@group({group})"))
}
