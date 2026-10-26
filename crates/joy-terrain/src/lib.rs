// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Terrain data shared by rendering, collision, and gameplay.
//!
//! - **Authored heightfields.** A version 1 [`TerrainAsset`] is one resident grid with fixed leaves; [`select_terrain`]
//!   and [`sample_terrain`] are its recorded reference behavior.
//! - **Tile pyramids.** A [`TerrainLayout`] describes a quadtree of decimated levels; [`build_terrain_pyramid`] and
//!   [`build_pyramid_from_asset`] build its [`TerrainTile`]s, and [`write_terrain_package`] stores them as a package a
//!   [`PackageTileLoader`] reads tile by tile, natively or over HTTP.
//! - **Surfaces.** A [`TerrainSurface`] is an immutable snapshot of resident tiles. The renderer draws it with
//!   [`select_terrain_nodes`] (quadtree continuous LOD), colliders build heightfields from it, and gameplay samples it
//!   with [`TerrainSurface::height_at`].
//! - **Streaming.** A [`TerrainStreamer`] keeps what views and collision regions need resident within a budget,
//!   synthesizes [`TerrainDetail`] below the authored resolution, applies [`TerrainCrater`] edits, and publishes
//!   snapshots.
//!
//! Coordinates are the terrain's local space: X and Z in world units, Y the height in world units, before any instance
//! transform. Nothing here allocates GPU resources or touches a physics solver, and every computation that feeds
//! gameplay uses portable arithmetic, so native and WebAssembly builds agree.

mod terrain_asset;
mod terrain_crater;
mod terrain_detail;
mod terrain_error;
mod terrain_grid;
mod terrain_layout;
mod terrain_loader;
mod terrain_pyramid;
mod terrain_quadtree;
mod terrain_selection;
mod terrain_streamer;
mod terrain_surface;
mod terrain_tile;

pub use terrain_asset::{
    TerrainAsset,
    TerrainBounds,
    TerrainManifest,
    TerrainPatch,
    decode_terrain,
    load_terrain,
    parse_terrain,
    sample_height_triangle,
    sample_terrain,
};
pub use terrain_crater::{
    CRATER_REACH_RADII,
    TerrainCrater,
};
pub use terrain_detail::{
    MAXIMUM_DETAIL_LEVELS,
    MAXIMUM_DETAIL_OCTAVES,
    TerrainDetail,
    TerrainDetailMaterial,
    TerrainMaterialRules,
    TerrainMaterialWeights,
};
pub use terrain_error::TerrainError;
pub use terrain_grid::{
    TerrainGrid,
    TerrainIndexRange,
    create_terrain_grid,
};
pub use terrain_layout::{
    MAXIMUM_LEVELS,
    MAXIMUM_TILE_CELLS,
    MINIMUM_TILE_CELLS,
    TILE_APRON,
    TerrainLayout,
    TileKey,
};
pub use terrain_loader::{
    MemoryTileLoader,
    PACKAGE_FORMAT_VERSION,
    PACKAGE_MANIFEST_FILE,
    PackageTileLoader,
    TerrainByteSource,
    TerrainPackage,
    TerrainTileLoad,
    TerrainTileLoader,
    write_terrain_package,
};
pub use terrain_pyramid::{
    build_pyramid_from_asset,
    build_terrain_pyramid,
};
pub use terrain_quadtree::{
    PREFETCH_DETAIL_MARGIN,
    TerrainNode,
    TerrainNodeSelection,
    coarsen_terrain_nodes,
    sample_node_height,
    select_terrain_nodes,
};
pub use terrain_selection::{
    TerrainSelection,
    TerrainView,
    reconcile_terrain_edges,
    sample_morphed_height,
    select_terrain,
    validate_terrain_view,
};
pub use terrain_streamer::{
    TerrainInterest,
    TerrainStreamer,
    TerrainStreamerOptions,
    TerrainStreamingBudget,
    TerrainStreamingReport,
};
pub use terrain_surface::{
    TerrainRayHit,
    TerrainSurface,
};
pub use terrain_tile::{
    TILE_FILE_MAGIC,
    TILE_FILE_VERSION,
    TerrainTile,
    decode_tile,
    encode_tile,
};
