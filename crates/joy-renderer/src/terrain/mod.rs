// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene terrain drawing: paged quadtree continuous LOD (CLOD) over a `joy_terrain::TerrainSurface`.
//!
//! The renderer draws whatever tiles the instance's surface snapshot holds. Each frame it selects tiles on the CPU
//! (`joy_terrain::select_terrain_nodes`, in the terrain's local space), keeps for each view (the camera and every shadow
//! view) the tiles whose bounds reach it, uploads any drawn tile it has not seen at its current revision into a pooled
//! height texture array, and writes one record per drawn tile per view. Every tile reuses one shared indexed lattice
//! and each view draws one instanced range per LOD; a record names the tile's texture layer, its placement, its
//! fractional detail, and its reconciled edge details, so the vertex shader morphs heights continuously toward the
//! parent lattice and shared edges agree across tiles of different levels. The same path runs on WebGPU, native
//! backends, and WebGL 2.
//!
//! Terrain always draws as part of a scene, through the same surface passes as meshes: its lit pipelines compose the
//! engine's surface entry points with a terrain shading model (lit-compatible, shaded by the smooth normal of the
//! morphed surface), so lighting, shadows, the G-buffer, scene-effect attachments, and debug views treat it like any
//! surface. The renderer draws it into the depth pre-pass and the lit or G-buffer pass (first among opaque geometry, as
//! the largest occluder) and into every shadow view it reaches. Gameplay and collision read the surface itself
//! (`TerrainSurface::height_at`), never camera-selected topology.

mod terrain_geometry;
pub(crate) mod terrain_scene_pass;
mod terrain_tile_layers;

pub(crate) use terrain_geometry::TerrainShadowView;
pub use terrain_geometry::{
    TerrainDiagnostic,
    TerrainStats,
};

use crate::pipelines::gbuffer_module_source;

// WGSL sources. Terrain shaders are the shared geometry file followed by their own entry points.
const TERRAIN_GEOMETRY_SOURCE: &str = include_str!("../shaders/terrain-geometry.wgsl");
const TERRAIN_MATERIAL_SOURCE: &str = include_str!("../shaders/terrain-material.wgsl");
const TERRAIN_SCENE_SOURCE: &str = include_str!("../shaders/terrain-scene.wgsl");
const TERRAIN_CASTER_SOURCE: &str = include_str!("../shaders/terrain-caster.wgsl");
const TERRAIN_DEPTH_SOURCE: &str = include_str!("../shaders/terrain-depth.wgsl");

/// The terrain group in the lit pipelines: the material group, after the scene pass's frame, shadow, and
/// image-lighting groups.
pub(crate) const TERRAIN_LIT_GROUP: u32 = 3;

/// The vertex entry point of the lit terrain module.
pub(crate) const TERRAIN_LIT_VERTEX_ENTRY: &str = "terrainVertexMain";

/// The shared geometry source with its bindings moved to `group`.
///
/// Each pipeline puts the terrain group right after its own groups, so no layout needs empty slots (which not every
/// browser's WebGPU accepts).
fn terrain_geometry_source(group: u32) -> String {
    TERRAIN_GEOMETRY_SOURCE.replace("@group(3)", &format!("@group({group})"))
}

/// The lit terrain module for the scene sources `common`: every surface pass entry point (forward, attachments, and
/// G-buffer) with the terrain geometry and shading model as the material, and the terrain vertex stage.
fn terrain_lit_source(common: &str) -> String {
    let model = format!("{}\n{TERRAIN_MATERIAL_SOURCE}", terrain_geometry_source(TERRAIN_LIT_GROUP));
    format!("{}\n{TERRAIN_SCENE_SOURCE}", gbuffer_module_source(common, &model))
}
