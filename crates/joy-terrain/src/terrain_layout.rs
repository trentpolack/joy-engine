// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The geometry of a terrain tile pyramid: how tiles at every level map onto the finest sample lattice and the
//! terrain's local space.
//!
//! Level 0 holds the finest authored samples. Each coarser level decimates the one below it by two, so a level-`L` tile
//! of `tile_cells` cells spans `tile_cells * 2^L` finest samples and its lattice is exactly the finest lattice at stride
//! `2^L`. Decimation keeps every coarse sample an authored sample: switching levels never moves a surviving vertex, and
//! any pointwise edit applies identically at every level. The top level holds the root tiles, which are always
//! resident. Negative levels are synthesized from level 0 (see `terrain_detail`); they halve the spacing again for each
//! level below zero.
//!
//! Local space is the terrain's own space before its instance transform: X and Z in world units with the finest sample
//! (0, 0) at `origin`, and Y the height in world units.

use joy_core::math::deterministic;
use serde::{
    Deserialize,
    Serialize,
};

use crate::terrain_asset::TerrainBounds;
use crate::terrain_error::TerrainError;

/// Smallest and largest cells per tile side (powers of two).
pub const MINIMUM_TILE_CELLS: u32 = 2;
/// Largest cells per tile side; patch records store at most eight error levels per tile.
pub const MAXIMUM_TILE_CELLS: u32 = 128;
/// Largest number of stored pyramid levels.
pub const MAXIMUM_LEVELS: u32 = 16;
/// Largest finest-sample index along either axis. Sample coordinates stay exact integers in `f32` shaders.
pub const MAXIMUM_SAMPLES_PER_AXIS: u64 = 1 << 24;
/// Samples added around each stored tile on every side, so cubic reconstruction near an edge never reads a neighbor.
pub const TILE_APRON: u32 = 1;

/// One tile of the pyramid: its level and its column (`x`) and row (`z`) at that level.
///
/// Level-`L` tiles are numbered from the pyramid's negative X/Z corner; a level holds `root_tiles * 2^(top - L)` tiles
/// along each axis. Negative levels name synthesized detail tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TileKey {
    /// Pyramid level; 0 is the finest authored level and negative levels are synthesized.
    pub level: i32,
    /// Column along +X at this level.
    pub x: u32,
    /// Row along +Z at this level.
    pub z: u32,
}

impl TileKey {
    /// A tile key.
    pub const fn new(level: i32, x: u32, z: u32) -> Self {
        Self {
            level,
            x,
            z,
        }
    }

    /// The tile one level up that contains this one.
    pub fn parent(self) -> Self {
        Self::new(self.level + 1, self.x / 2, self.z / 2)
    }

    /// The four tiles one level down, in row-major order (-Z row first, -X first).
    pub fn children(self) -> [Self; 4] {
        let (x, z, level) = (self.x * 2, self.z * 2, self.level - 1);
        [Self::new(level, x, z), Self::new(level, x + 1, z), Self::new(level, x, z + 1), Self::new(level, x + 1, z + 1)]
    }

    /// The level-`level` tile containing this one; `level` must not be finer than this tile.
    pub fn ancestor(self, level: i32) -> Self {
        let shift = (level - self.level) as u32;
        Self::new(level, self.x >> shift, self.z >> shift)
    }
}

/// Geometry and quantization of a tile pyramid. Plain data; validate with [`TerrainLayout::validate`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainLayout {
    /// Cells per tile side at every level; a power of two from 2 to 128.
    pub tile_cells: u32,
    /// Stored (authored) levels; level `levels - 1` holds the root tiles.
    pub levels: u32,
    /// Root tiles along X and Z.
    pub root_tiles: [u32; 2],
    /// Local X/Z position of finest sample (0, 0), in world units.
    pub origin: [f64; 2],
    /// Local distance between neighboring finest samples along X and Z, in world units.
    pub spacing: [f64; 2],
    /// World height of quantized sample zero in tile files.
    pub height_offset: f64,
    /// World height range spanned by quantized tile samples (`offset + (sample / 65535) * scale`).
    pub height_scale: f64,
}

impl TerrainLayout {
    /// Check every field; fails with the first invalid one.
    pub fn validate(&self) -> Result<(), TerrainError> {
        if !self.tile_cells.is_power_of_two() || !(MINIMUM_TILE_CELLS..=MAXIMUM_TILE_CELLS).contains(&self.tile_cells) {
            return Err(TerrainError::InvalidLayout("tileCells must be a power of two from 2 to 128"));
        }
        if !(1..=MAXIMUM_LEVELS).contains(&self.levels) {
            return Err(TerrainError::InvalidLayout("levels must be between 1 and 16"));
        }
        if self.root_tiles.contains(&0) {
            return Err(TerrainError::InvalidLayout("rootTiles must be positive"));
        }
        let samples = self.finest_cells();
        if samples[0] >= MAXIMUM_SAMPLES_PER_AXIS || samples[1] >= MAXIMUM_SAMPLES_PER_AXIS {
            return Err(TerrainError::InvalidLayout("the finest lattice must hold fewer than 2^24 samples per axis"));
        }
        if !self.origin.iter().all(|value| value.is_finite()) {
            return Err(TerrainError::InvalidLayout("origin must be finite"));
        }
        if !self.spacing.iter().all(|value| value.is_finite() && *value > 0.0) {
            return Err(TerrainError::InvalidLayout("spacing must be finite and positive"));
        }
        if !self.height_offset.is_finite() || !self.height_scale.is_finite() || self.height_scale < 0.0 {
            return Err(TerrainError::InvalidLayout("height offset and scale must be finite with a nonnegative scale"));
        }
        Ok(())
    }

    /// The root level (`levels - 1`).
    pub fn top_level(&self) -> i32 {
        self.levels as i32 - 1
    }

    /// `log2(tile_cells)`: how many power-of-two strides one tile spans beyond its own lattice.
    pub fn cells_log2(&self) -> i32 {
        self.tile_cells.trailing_zeros() as i32
    }

    /// The coarsest global detail level: a root tile drawn as two triangles.
    pub fn coarsest_detail(&self) -> i32 {
        self.top_level() + self.cells_log2()
    }

    /// Finest cells along X and Z (one less than the finest samples).
    pub fn finest_cells(&self) -> [u64; 2] {
        let per_root = u64::from(self.tile_cells) << (self.levels - 1);
        [u64::from(self.root_tiles[0]) * per_root, u64::from(self.root_tiles[1]) * per_root]
    }

    /// Tiles along X and Z at `level`; zero for levels above the roots.
    pub fn tiles_at(&self, level: i32) -> [u64; 2] {
        let top = self.top_level();
        // Levels more than 40 below the roots would overflow the counts; no supported pyramid reaches them.
        if level > top || (top - level) > 40 {
            return [0, 0];
        }
        let shift = (top - level) as u32;
        [u64::from(self.root_tiles[0]) << shift, u64::from(self.root_tiles[1]) << shift]
    }

    /// Whether `key` names a tile inside the pyramid at a level no finer than `finest_level`.
    pub fn contains(&self, key: TileKey, finest_level: i32) -> bool {
        let tiles = self.tiles_at(key.level);
        key.level >= finest_level && u64::from(key.x) < tiles[0] && u64::from(key.z) < tiles[1]
    }

    /// The root tiles in row-major order (-Z row first).
    pub fn roots(&self) -> impl Iterator<Item = TileKey> + '_ {
        let top = self.top_level();
        (0..self.root_tiles[1]).flat_map(move |z| (0..self.root_tiles[0]).map(move |x| TileKey::new(top, x, z)))
    }

    /// Finest samples spanned by one tile side at `level` (`tile_cells * 2^level`; fractional below zero).
    pub fn tile_span_samples(&self, level: i32) -> f64 {
        f64::from(self.tile_cells) * deterministic::exp2(f64::from(level))
    }

    /// The finest-lattice coordinates of a tile's (0, 0) corner.
    pub fn tile_origin_samples(&self, key: TileKey) -> [f64; 2] {
        let span = self.tile_span_samples(key.level);
        [f64::from(key.x) * span, f64::from(key.z) * span]
    }

    /// Local X/Z of a finest-lattice coordinate (fractional coordinates allowed).
    pub fn sample_to_local(&self, sample_x: f64, sample_z: f64) -> [f64; 2] {
        [self.origin[0] + (sample_x * self.spacing[0]), self.origin[1] + (sample_z * self.spacing[1])]
    }

    /// The finest-lattice coordinate of a local X/Z position (unclamped).
    pub fn local_to_sample(&self, x: f64, z: f64) -> [f64; 2] {
        [(x - self.origin[0]) / self.spacing[0], (z - self.origin[1]) / self.spacing[1]]
    }

    /// A tile's local X/Z footprint as `(minimum, maximum)` corners.
    pub fn tile_footprint(&self, key: TileKey) -> ([f64; 2], [f64; 2]) {
        let origin = self.tile_origin_samples(key);
        let span = self.tile_span_samples(key.level);
        (self.sample_to_local(origin[0], origin[1]), self.sample_to_local(origin[0] + span, origin[1] + span))
    }

    /// The larger finest sample spacing; LOD selection limits projected cells with it.
    pub fn maximum_spacing(&self) -> f64 {
        self.spacing[0].max(self.spacing[1])
    }

    /// Samples per stored tile side, including the apron on both sides.
    pub fn stored_tile_side(&self) -> u32 {
        self.tile_cells + 1 + (2 * TILE_APRON)
    }

    /// The local bounds of the whole pyramid footprint with the given height range.
    pub fn footprint_bounds(&self, minimum_height: f64, maximum_height: f64) -> TerrainBounds {
        let cells = self.finest_cells();
        let low = self.sample_to_local(0.0, 0.0);
        let high = self.sample_to_local(cells[0] as f64, cells[1] as f64);
        TerrainBounds {
            min: joy_core::math::DVec3::new(low[0], minimum_height, low[1]),
            max: joy_core::math::DVec3::new(high[0], maximum_height, high[1]),
        }
    }

    /// Decode one quantized tile-file sample into world height.
    pub fn decode_height(&self, quantized: u16) -> f64 {
        self.height_offset + ((f64::from(quantized) / 65535.0) * self.height_scale)
    }

    /// Quantize a world height into the tile-file range, saturating outside it.
    pub fn encode_height(&self, height: f64) -> u16 {
        if self.height_scale <= 0.0 {
            return 0;
        }
        let normalized = ((height - self.height_offset) / self.height_scale).clamp(0.0, 1.0);
        (normalized * 65535.0).round() as u16
    }
}
