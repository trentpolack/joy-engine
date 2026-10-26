// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use joy_core::math::deterministic;
use joy_terrain::{
    TerrainLayout,
    TerrainSurface,
    TileKey,
    build_terrain_pyramid,
};

use super::*;

/// Three levels of 8-cell tiles under 2 x 2 roots over a sloped, rippled grid, so every height is distinct.
fn surface() -> (TerrainSurface, Vec<f64>, usize) {
    let layout = TerrainLayout {
        tile_cells: 8,
        levels: 3,
        root_tiles: [2, 2],
        origin: [0.0, 0.0],
        spacing: [1.0, 1.0],
        height_offset: -64.0,
        height_scale: 256.0,
    };
    let columns = 65;
    let heights: Vec<f64> = (0..columns * columns)
        .map(|index| {
            let (x, z) = ((index % columns) as f64, (index / columns) as f64);
            (0.5 * x) + (0.25 * z) + (3.0 * deterministic::sin(x * 0.3) * deterministic::cos(z * 0.2))
        })
        .collect();
    let tiles = build_terrain_pyramid(&layout, &heights).unwrap();
    (TerrainSurface::from_tiles(layout, tiles, Default::default()).unwrap(), heights, columns)
}

/// The ring texel the shader's `latticeHeight` reads for point (`x`, `z`) one stride `2^k` beyond a tile's side.
fn shader_ring_texel(texels: &[f32], cells: i32, k: i32, x: i32, z: i32) -> f32 {
    let (side, along) = if x < 0 {
        (0, z)
    } else if x > cells {
        (1, z)
    } else if z < 0 {
        (2, x)
    } else {
        (3, x)
    };
    let count = (cells >> k) + 1;
    let previous = 4 * ((cells - (cells >> (k - 1))) + (k - 1));
    let offset = previous + (side * count) + (along >> k);
    let width = cells + 3;
    texels[((width + (offset / width)) * width + (offset % width)) as usize]
}

#[test]
fn layers_hold_the_lattice_beyond_every_edge_at_every_stride() {
    let (surface, heights, columns) = surface();
    let finest = |x: i64, z: i64| heights[(z as usize * columns) + x as usize] as f32;
    let cells = 8;
    let mut texels = Vec::new();

    // An interior level-0 tile and a level-1 tile: every apron and ring point inside the terrain is the finest sample
    // under it, read from whichever tile holds it.
    for key in [TileKey::new(0, 3, 4), TileKey::new(1, 1, 2)] {
        let tile = surface.tile(key).unwrap();
        build_tile_layer(&surface, tile, &mut texels);
        let step = 1_i64 << key.level;
        let origin = [i64::from(key.x) * cells * step, i64::from(key.z) * cells * step];
        let at = |x: i32, z: i32| (origin[0] + (i64::from(x) * step), origin[1] + (i64::from(z) * step));
        let inside = |(x, z): (i64, i64)| (0..columns as i64).contains(&x) && (0..columns as i64).contains(&z);
        let width = cells as i32 + 3;
        for z in -1..=(cells as i32 + 1) {
            for x in -1..=(cells as i32 + 1) {
                let point = at(x, z);
                if inside(point) {
                    assert_eq!(
                        texels[((z + 1) * width + (x + 1)) as usize],
                        finest(point.0, point.1),
                        "{key:?} {x},{z}"
                    );
                }
            }
        }
        for k in 1..=3 {
            let stride = 1 << k;
            for along in (0..=cells as i32).step_by(stride as usize) {
                for (x, z) in [(-stride, along), (8 + stride, along), (along, -stride), (along, 8 + stride)] {
                    let point = at(x, z);
                    if inside(point) {
                        let texel = shader_ring_texel(&texels, cells as i32, k, x, z);
                        assert_eq!(texel, finest(point.0, point.1), "{key:?} ring {k} at {x},{z}");
                    }
                }
            }
        }
    }
}
