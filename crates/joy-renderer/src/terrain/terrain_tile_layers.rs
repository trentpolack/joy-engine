// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The texels one tile uploads into its layer of the terrain height pool.
//!
//! A layer holds the tile's lattice with a one-sample apron, then "rings": for each stride `2^k` from 2 up to the tile's
//! width, and for each side (-X, +X, -Z, +Z), the lattice heights one stride beyond that side at every stride-`2^k`
//! point along it. The vertex shader takes central differences at whatever stride a vertex draws, so normals at tile
//! edges read the real heights across them and agree with every neighbor, of any level, at every shared point.
//! terrain-geometry.wgsl reads this layout (`latticeHeight`).
//!
//! Every level samples one surface at the lattice points it shares with the others (coarse samples are the finest
//! samples beneath them, and detail vanishes on the authored lattice), so a height outside the tile can come from any
//! resident tile that has that lattice point: a same-level neighbor, or a coarser tile whose lattice reaches it. Beyond
//! the terrain's edge the surface continues its slope.

use joy_terrain::{
    TerrainLayout,
    TerrainSurface,
    TerrainTile,
    TileKey,
};

/// Texels per layer row: the tile lattice and its one-sample apron on both sides.
pub(crate) fn layer_width(cells: u32) -> u32 {
    cells + 3
}

/// Rows per layer: the apron-padded lattice, then the rings packed row after row.
pub(crate) fn layer_height(cells: u32) -> u32 {
    let width = layer_width(cells);
    width + ring_texels(cells).div_ceil(width)
}

/// Ring heights per tile: four sides of `cells / 2^k + 1` points for every stride `2^k` from 2 to `cells`.
fn ring_texels(cells: u32) -> u32 {
    4 * ((cells - 1) + cells.trailing_zeros())
}

/// Fill `texels` (row-major, [`layer_width`] by [`layer_height`]) with `tile`'s layer.
pub(crate) fn build_tile_layer(surface: &TerrainSurface, tile: &TerrainTile, texels: &mut Vec<f32>) {
    let layout = surface.layout();
    let cells = layout.tile_cells as i32;
    let width = layer_width(layout.tile_cells) as usize;
    texels.clear();
    texels.resize(width * layer_height(layout.tile_cells) as usize, 0.0);

    // The lattice itself, then the apron from the neighbors that own those points (the tile's own apron otherwise).
    let key = tile.key();
    let lattice = TileLattice {
        surface,
        layout,
        key,
        origin: [i64::from(key.x) * i64::from(cells), i64::from(key.z) * i64::from(cells)],
    };
    for z in -1..=(cells + 1) {
        for x in -1..=(cells + 1) {
            let inside = (0..=cells).contains(&x) && (0..=cells).contains(&z);
            let height = if inside { None } else { lattice.outside(x, z) };
            texels[(((z + 1) as usize) * width) + (x + 1) as usize] = height.unwrap_or(tile.height(x, z));
        }
    }

    // Rings, in the order the shader indexes them: by stride, then side, then position along the side.
    let mut offset = width * width;
    for k in 1..=layout.cells_log2() {
        let stride = 1 << k;
        for side in 0..4 {
            for along in (0..=cells).step_by(stride as usize) {
                // The ring point, the tile's edge point it lies beyond, and the point one stride back inside.
                let (point, edge, back) = match side {
                    0 => ([-stride, along], [0, along], [stride, along]),
                    1 => ([cells + stride, along], [cells, along], [cells - stride, along]),
                    2 => ([along, -stride], [along, 0], [along, stride]),
                    _ => ([along, cells + stride], [along, cells], [along, cells - stride]),
                };

                // Past the terrain's edge the surface continues the slope of its last stride.
                texels[offset] = lattice
                    .outside(point[0], point[1])
                    .unwrap_or_else(|| (2.0 * tile.height(edge[0], edge[1])) - tile.height(back[0], back[1]));
                offset += 1;
            }
        }
    }
}

/// Lattice lookups around one tile, in its level's global lattice coordinates.
struct TileLattice<'a> {
    surface: &'a TerrainSurface,
    layout: &'a TerrainLayout,
    key: TileKey,
    origin: [i64; 2],
}

impl TileLattice<'_> {
    /// The height at tile-lattice point (`x`, `z`) outside the tile, from whichever resident tile has that lattice point;
    /// `None` beyond the terrain or where nothing resident reaches it.
    fn outside(&self, x: i32, z: i32) -> Option<f32> {
        lattice_height(
            self.surface,
            self.layout,
            self.key.level,
            self.origin[0] + i64::from(x),
            self.origin[1] + i64::from(z),
        )
    }
}

/// The height at global lattice point (`x`, `z`) of `level`, searching that level and then each coarser level whose
/// lattice includes the point. A tile that holds the point inside its footprint wins over one that holds it only in its
/// apron.
fn lattice_height(surface: &TerrainSurface, layout: &TerrainLayout, level: i32, x: i64, z: i64) -> Option<f32> {
    let cells = i64::from(layout.tile_cells);
    let (mut x, mut z) = (x, z);
    let mut apron = None;
    for current in level..=layout.top_level() {
        let tiles = layout.tiles_at(current);
        let (extent_x, extent_z) = (tiles[0] as i64 * cells, tiles[1] as i64 * cells);
        if x < 0 || z < 0 || x > extent_x || z > extent_z {
            return apron;
        }
        for tile_z in tile_range(z, cells, tiles[1] as i64) {
            for tile_x in tile_range(x, cells, tiles[0] as i64) {
                let Some(tile) = surface.tile(TileKey::new(current, tile_x as u32, tile_z as u32)) else {
                    continue;
                };
                let (local_x, local_z) = (x - (tile_x * cells), z - (tile_z * cells));
                let height = tile.height(local_x as i32, local_z as i32);
                if (0..=cells).contains(&local_x) && (0..=cells).contains(&local_z) {
                    return Some(height);
                }
                apron.get_or_insert(height);
            }
        }

        // The next level's lattice holds only even points.
        if x % 2 != 0 || z % 2 != 0 {
            return apron;
        }
        x /= 2;
        z /= 2;
    }
    apron
}

/// Tiles along one axis whose footprint or apron holds lattice coordinate `coordinate`.
fn tile_range(coordinate: i64, cells: i64, count: i64) -> impl Iterator<Item = i64> {
    let first = (coordinate - 1).div_euclid(cells).max(0);
    let last = (coordinate + 1).div_euclid(cells).min(count - 1);
    first..=last
}

/// Whether a tile's layer may read `changed`: a tile at its level or coarser within one tile span of its footprint
/// (the reach of its widest ring).
pub(crate) fn layer_reads(layout: &TerrainLayout, key: TileKey, changed: TileKey) -> bool {
    if changed.level < key.level {
        return false;
    }
    let (low, high) = layout.tile_footprint(key);
    let span = high[0] - low[0];
    let (changed_low, changed_high) = layout.tile_footprint(changed);
    changed_low[0] <= high[0] + span
        && changed_high[0] >= low[0] - span
        && changed_low[1] <= high[1] + span
        && changed_high[1] >= low[1] - span
}

#[cfg(test)]
#[path = "terrain_tile_layers.test.rs"]
mod tests;
