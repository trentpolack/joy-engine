// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Pyramid construction: level-0 measurement matches the version 1 offline compiler, and coarse tiles decimate the
//! finest lattice with bounds and errors that really bound it.

use joy_terrain::{
    TerrainLayout,
    TileKey,
    build_pyramid_from_asset,
    build_terrain_pyramid,
};

use crate::{
    dragon_pit,
    rolling_pyramid,
};

/// Measured errors agree with the compiler's to this many world units (both read the same decoded samples).
const MEASUREMENT_TOLERANCE: f64 = 1e-9;
/// Tiles store `f32` samples; checks against them allow that rounding.
const SAMPLE_TOLERANCE: f64 = 1e-4;

#[test]
fn level_zero_measurement_matches_the_version_one_compiler() {
    let asset = dragon_pit();
    let (layout, _) = build_pyramid_from_asset(&asset, 1).expect("one level");
    let heights: Vec<f64> = (0..asset.heights().len()).map(|index| asset.decoded_height(index)).collect();
    let measured = build_terrain_pyramid(&layout, &heights).expect("the decoded grid is valid");

    assert_eq!(measured.len(), asset.patches().len());
    for (tile, patch) in measured.iter().zip(asset.patches()) {
        assert_eq!(tile.key(), TileKey::new(0, patch.x / 64, patch.z / 64));
        assert!((tile.minimum() - patch.min).abs() < MEASUREMENT_TOLERANCE);
        assert!((tile.maximum() - patch.max).abs() < MEASUREMENT_TOLERANCE);
        for (error, recorded) in tile.errors().iter().zip(&patch.errors) {
            assert!((error - recorded).abs() < MEASUREMENT_TOLERANCE, "{:?}: {error} != {recorded}", tile.key());
        }
    }
}

#[test]
fn coarse_tiles_decimate_and_bound_the_finest_surface() {
    let (layout, tiles) = rolling_pyramid(3);
    let finest = finest_grid(&layout, &tiles);
    let columns = layout.finest_cells()[0] as usize + 1;
    let at = |x: usize, z: usize| finest[(z * columns) + x];

    for tile in tiles.iter().filter(|tile| tile.key().level > 0) {
        let key = tile.key();
        let stride = 1_usize << key.level;
        let span = 8 * stride;
        let (origin_x, origin_z) = (key.x as usize * span, key.z as usize * span);

        // Every coarse sample is the finest sample beneath it.
        for z in 0..=8 {
            for x in 0..=8 {
                assert_eq!(
                    tile.height(x, z),
                    at(origin_x + (x as usize * stride), origin_z + (z as usize * stride)) as f32
                );
            }
        }

        // Bounds hold every finest sample, and each stride's error bounds its triangles' miss of the finest surface.
        for level in key.level..=(key.level + 3) {
            let step = 1_usize << level;
            let recorded = tile.errors()[level as usize];
            for z in 0..=span {
                for x in 0..=span {
                    let height = at(origin_x + x, origin_z + z);
                    assert!(height >= tile.minimum() - SAMPLE_TOLERANCE && height <= tile.maximum() + SAMPLE_TOLERANCE);
                    let base_x = ((x / step) * step).min(span - step);
                    let base_z = ((z / step) * step).min(span - step);
                    let (fx, fz) = ((x - base_x) as f64 / step as f64, (z - base_z) as f64 / step as f64);
                    let a = at(origin_x + base_x, origin_z + base_z);
                    let b = at(origin_x + base_x + step, origin_z + base_z);
                    let c = at(origin_x + base_x, origin_z + base_z + step);
                    let d = at(origin_x + base_x + step, origin_z + base_z + step);
                    let surface =
                        if fx >= fz { a + (b - a) * fx + (d - b) * fz } else { a + (d - c) * fx + (c - a) * fz };
                    assert!(
                        (height - surface).abs() <= recorded + SAMPLE_TOLERANCE,
                        "{key:?} level {level} misses its error"
                    );
                }
            }
        }
    }
}

/// The finest grid of a rolling pyramid, read back from its level-0 tiles (f32 samples widened to f64).
fn finest_grid(layout: &TerrainLayout, tiles: &[joy_terrain::TerrainTile]) -> Vec<f64> {
    let cells = layout.finest_cells();
    let columns = cells[0] as usize + 1;
    let mut grid = vec![0.0; columns * (cells[1] as usize + 1)];
    for tile in tiles.iter().filter(|tile| tile.key().level == 0) {
        for z in 0..=8 {
            for x in 0..=8 {
                let column = (tile.key().x as usize * 8) + x as usize;
                let row = (tile.key().z as usize * 8) + z as usize;
                grid[(row * columns) + column] = f64::from(tile.height(x, z));
            }
        }
    }
    grid
}
