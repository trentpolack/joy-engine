// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Integration tests for `joy-terrain`: version 1 assets against their golden recording, pyramid construction and
//! packages, quadtree selection seams and continuity, and streaming with detail synthesis and craters. Run with
//! `cargo test -p joy-terrain`; re-record the version 1 recording with `JOY_UPDATE_FIXTURES=1`.

mod terrain_asset;
mod terrain_package;
mod terrain_pyramid;
mod terrain_quadtree;
mod terrain_streaming;

use joy_core::math::{
    DVec3,
    deterministic,
};
use joy_terrain::{
    TerrainAsset,
    TerrainLayout,
    TerrainTile,
    TerrainView,
    build_terrain_pyramid,
    parse_terrain,
};

pub(crate) const DRAGON_PIT_MANIFEST: &str =
    include_str!("../../../../examples/terrain/assets/dragon-pit/terrain.json");
pub(crate) const DRAGON_PIT_HEIGHTS: &[u8] =
    include_bytes!("../../../../examples/terrain/assets/dragon-pit/height.bin");

/// The Dragon Pit example asset (1025 x 1025 samples, 64-cell leaves, 2048 x 2048 units).
pub(crate) fn dragon_pit() -> TerrainAsset {
    parse_terrain(DRAGON_PIT_MANIFEST, DRAGON_PIT_HEIGHTS).expect("the example asset is valid")
}

/// A rough synthetic layout: `levels` stored levels of 8-cell tiles under 2 x 2 roots, one world unit per sample,
/// centered on the origin.
pub(crate) fn rolling_layout(levels: u32) -> TerrainLayout {
    let cells = 8_u32 << (levels - 1);
    TerrainLayout {
        tile_cells: 8,
        levels,
        root_tiles: [2, 2],
        origin: [-f64::from(cells), -f64::from(cells)],
        spacing: [1.0, 1.0],
        height_offset: -64.0,
        height_scale: 256.0,
    }
}

/// Hills and ridges over a rolling layout's finest grid, so every level has real error.
pub(crate) fn rolling_pyramid(levels: u32) -> (TerrainLayout, Vec<TerrainTile>) {
    let layout = rolling_layout(levels);
    let cells = layout.finest_cells();
    let mut heights = Vec::new();
    for z in 0..=cells[1] {
        for x in 0..=cells[0] {
            let (x, z) = (x as f64, z as f64);
            let hills = 18.0 * deterministic::sin(x * 0.11) * deterministic::cos(z * 0.07);
            let ridges = 6.0 * deterministic::sin((x * 0.43) + (z * 0.31)).abs();
            heights.push(hills + ridges + (0.002 * x * z));
        }
    }
    let tiles = build_terrain_pyramid(&layout, &heights).expect("the rolling grid is valid");
    (layout, tiles)
}

/// A view from `eye` looking at `target`, with a 965.7 CSS-pixel projection scale and a two-pixel tolerance.
pub(crate) fn view(eye: [f64; 3], target: [f64; 3], target_radius: f64) -> TerrainView {
    TerrainView {
        eye: DVec3::from_array(eye),
        projection_scale: 965.7,
        tolerance: 2.0,
        target: DVec3::from_array(target),
        target_radius,
    }
}
