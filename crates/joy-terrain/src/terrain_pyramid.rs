// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Offline and load-time construction of tile pyramids from a finest-level height grid.
//!
//! Level-0 tiles copy the authored samples. Every coarser tile decimates the level below it, so its lattice is the
//! finest lattice at stride `2^level`. Bounds and errors always describe the finest surface inside a tile's footprint:
//! a parent's bounds and its errors for strides its children also draw are the maximum over its four children (exact,
//! because every lattice is aligned to the same finest grid), and only its own coarsest stride is measured directly.
//! Error measurement matches the version 1 offline compiler: each finest sample is compared with the stride's triangle
//! surface (the top-left to bottom-right diagonal split) inside the tile, then the table is made monotone.
//!
//! The whole finest grid is held in memory while building, which suits worlds up to a few thousand samples per side;
//! larger worlds need an out-of-core builder.

use crate::terrain_asset::TerrainAsset;
use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TILE_APRON,
    TerrainLayout,
    TileKey,
};
use crate::terrain_tile::TerrainTile;

/// Build every stored tile of `layout` from finest heights in world units.
///
/// `heights` is row-major (-Z row first) with `finest_cells + 1` samples along each axis. Tiles are returned coarsest
/// level first, row-major within a level, so a loader can serve roots before their descendants.
pub fn build_terrain_pyramid(layout: &TerrainLayout, heights: &[f64]) -> Result<Vec<TerrainTile>, TerrainError> {
    layout.validate()?;
    let grid = HeightGrid::new(layout, heights)?;

    // Level-0 bounds and errors are measured on the authored samples.
    let mut level_zero = Vec::new();
    for key in level_keys(layout, 0) {
        let span = layout.tile_cells as usize;
        let origin = [key.x as usize * span, key.z as usize * span];
        let (minimum, maximum) = grid.bounds(origin, span);
        let mut errors = vec![0.0];
        for log2_stride in 1..=layout.cells_log2() {
            let previous = errors[errors.len() - 1];
            errors.push(grid.triangle_error(origin, span, 1 << log2_stride).max(previous));
        }
        level_zero.push(Measured {
            key,
            minimum,
            maximum,
            errors,
        });
    }
    build_levels(layout, &grid, level_zero)
}

/// Build a pyramid of `levels` stored levels from a version 1 asset, keeping the asset's own patch bounds and errors
/// for level 0 so a one-level pyramid selects exactly like the asset.
///
/// The asset's leaves per side must divide into `2^(levels - 1)` root columns. Returns the layout (centered on the
/// asset like [`crate::sample_terrain`]) and the tiles, coarsest level first.
pub fn build_pyramid_from_asset(
    asset: &TerrainAsset,
    levels: u32,
) -> Result<(TerrainLayout, Vec<TerrainTile>), TerrainError> {
    let manifest = asset.manifest();
    let leaves = asset.tiles_per_axis();
    if levels == 0 || levels > 16 || !leaves.is_multiple_of(1 << (levels - 1)) {
        return Err(TerrainError::InvalidLayout("the asset's leaves per side must divide into 2^(levels - 1) roots"));
    }
    let cells = f64::from(manifest.size - 1);
    let layout = TerrainLayout {
        tile_cells: manifest.tile_cells,
        levels,
        root_tiles: [leaves >> (levels - 1), leaves >> (levels - 1)],
        origin: [-(manifest.extent[0] / 2.0), -(manifest.extent[1] / 2.0)],
        spacing: [manifest.extent[0] / cells, manifest.extent[1] / cells],
        height_offset: manifest.height_offset,
        height_scale: manifest.height_scale,
    };
    let heights: Vec<f64> = (0..asset.heights().len()).map(|index| asset.decoded_height(index)).collect();
    let grid = HeightGrid::new(&layout, &heights)?;

    // Asset patches are row-major leaves, which are exactly the level-0 tiles.
    let level_zero = asset
        .patches()
        .iter()
        .map(|patch| {
            let key = TileKey::new(0, patch.x / manifest.tile_cells, patch.z / manifest.tile_cells);
            Measured {
                key,
                minimum: patch.min,
                maximum: patch.max,
                errors: patch.errors.clone(),
            }
        })
        .collect();
    let tiles = build_levels(&layout, &grid, level_zero)?;
    Ok((layout, tiles))
}

/// Tiles of every level from level-0 measurements, coarsest level first.
fn build_levels(
    layout: &TerrainLayout,
    grid: &HeightGrid<'_>,
    level_zero: Vec<Measured>,
) -> Result<Vec<TerrainTile>, TerrainError> {
    // Measurements per level, row-major, finest level first while building.
    let mut levels: Vec<Vec<Measured>> = vec![level_zero];
    for level in 1..=layout.top_level() {
        let below = &levels[(level - 1) as usize];
        let columns_below = layout.tiles_at(level - 1)[0] as usize;
        let mut measured = Vec::new();
        for key in level_keys(layout, level) {
            let children = key.children().map(|child| &below[(child.z as usize * columns_below) + child.x as usize]);
            let minimum = children.iter().map(|child| child.minimum).fold(f64::INFINITY, f64::min);
            let maximum = children.iter().map(|child| child.maximum).fold(f64::NEG_INFINITY, f64::max);

            // Strides the children also draw take their maximum; only this tile's two-triangle stride is new.
            let shared = children[0].errors.len();
            let mut errors: Vec<f64> =
                (0..shared).map(|index| children.iter().map(|child| child.errors[index]).fold(0.0, f64::max)).collect();
            let span = (layout.tile_cells as usize) << level;
            let origin = [key.x as usize * span, key.z as usize * span];
            errors.push(grid.triangle_error(origin, span, span).max(errors[shared - 1]));
            measured.push(Measured {
                key,
                minimum,
                maximum,
                errors,
            });
        }
        levels.push(measured);
    }

    let mut tiles = Vec::new();
    for measured in levels.into_iter().rev() {
        for tile in measured {
            let heights = grid.tile_heights(layout, tile.key);
            tiles.push(TerrainTile::new(layout, tile.key, heights, tile.minimum, tile.maximum, 0, tile.errors)?);
        }
    }
    Ok(tiles)
}

/// One tile's measured finest-surface bounds and errors, before its samples are gathered.
struct Measured {
    key: TileKey,
    minimum: f64,
    maximum: f64,
    errors: Vec<f64>,
}

/// Every tile key of one stored level, row-major.
fn level_keys(layout: &TerrainLayout, level: i32) -> impl Iterator<Item = TileKey> {
    let tiles = layout.tiles_at(level);
    (0..tiles[1] as u32).flat_map(move |z| (0..tiles[0] as u32).map(move |x| TileKey::new(level, x, z)))
}

/// A borrowed finest-level grid with clamped reads.
struct HeightGrid<'a> {
    columns: usize,
    rows: usize,
    heights: &'a [f64],
}

impl<'a> HeightGrid<'a> {
    fn new(layout: &TerrainLayout, heights: &'a [f64]) -> Result<Self, TerrainError> {
        let cells = layout.finest_cells();
        let (columns, rows) = (cells[0] as usize + 1, cells[1] as usize + 1);
        if heights.len() != columns * rows || !heights.iter().all(|height| height.is_finite()) {
            return Err(TerrainError::InvalidDimensions);
        }
        Ok(Self {
            columns,
            rows,
            heights,
        })
    }

    /// The height at a finest coordinate, clamped to the grid (aprons at the world's edge repeat the border).
    fn at(&self, column: i64, row: i64) -> f64 {
        let column = column.clamp(0, self.columns as i64 - 1) as usize;
        let row = row.clamp(0, self.rows as i64 - 1) as usize;
        self.heights[(row * self.columns) + column]
    }

    /// Apron-padded tile samples at the tile level's stride, as world heights.
    fn tile_heights(&self, layout: &TerrainLayout, key: TileKey) -> Vec<f32> {
        let stride = 1_i64 << key.level;
        let span = i64::from(layout.tile_cells) * stride;
        let (origin_x, origin_z) = (i64::from(key.x) * span, i64::from(key.z) * span);
        let apron = i64::from(TILE_APRON);
        let mut heights = Vec::with_capacity((layout.stored_tile_side() as usize).pow(2));
        for row in -apron..=(i64::from(layout.tile_cells) + apron) {
            for column in -apron..=(i64::from(layout.tile_cells) + apron) {
                heights.push(self.at(origin_x + (column * stride), origin_z + (row * stride)) as f32);
            }
        }
        heights
    }

    /// Smallest and largest finest samples in a square footprint of `span` cells.
    fn bounds(&self, origin: [usize; 2], span: usize) -> (f64, f64) {
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        for row in origin[1]..=origin[1] + span {
            for column in origin[0]..=origin[0] + span {
                let height = self.heights[(row * self.columns) + column];
                minimum = minimum.min(height);
                maximum = maximum.max(height);
            }
        }
        (minimum, maximum)
    }

    /// Largest difference between the finest samples of a footprint and its triangle surface at `stride`.
    fn triangle_error(&self, origin: [usize; 2], span: usize, stride: usize) -> f64 {
        let at = |x: usize, z: usize| self.heights[((origin[1] + z) * self.columns) + origin[0] + x];
        let mut largest: f64 = 0.0;
        for z in 0..=span {
            for x in 0..=span {
                // The final cell absorbs the far edge, as the renderer's lattice does.
                let base_x = ((x / stride) * stride).min(span - stride);
                let base_z = ((z / stride) * stride).min(span - stride);
                let fraction_x = (x - base_x) as f64 / stride as f64;
                let fraction_z = (z - base_z) as f64 / stride as f64;
                let a = at(base_x, base_z);
                let b = at(base_x + stride, base_z);
                let c = at(base_x, base_z + stride);
                let d = at(base_x + stride, base_z + stride);
                let approximation = if fraction_x >= fraction_z {
                    a + ((b - a) * fraction_x) + ((d - b) * fraction_z)
                } else {
                    a + ((d - c) * fraction_x) + ((c - a) * fraction_z)
                };
                largest = largest.max((at(x, z) - approximation).abs());
            }
        }
        largest
    }
}
