// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One decoded pyramid tile and its portable binary file format.
//!
//! A tile stores its lattice as world heights with a one-sample apron on every side, the height bounds of the finest
//! surface inside its footprint, and a monotone error table: entry `i` is the largest height error, in world units, of
//! drawing the footprint with the lattice at global level `first_error_level + i` (stride `2^level` finest samples)
//! instead of the finest surface. Tiles are immutable once built; edits produce new tiles with a newer revision.
//!
//! Tile files (`.jtile`) are little endian: a 40-byte header (`JTIL`, format version, cells, level, column, row, error
//! count, minimum and maximum height), the `f64` errors, then the quantized `u16` samples row by row, -Z row first,
//! apron included. Heights decode with the layout's global offset and scale, so neighboring tiles never disagree.

use std::sync::Arc;

use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TILE_APRON,
    TerrainLayout,
    TileKey,
};

/// The four bytes that open every tile file.
pub const TILE_FILE_MAGIC: [u8; 4] = *b"JTIL";
/// The tile file format version this crate reads and writes.
pub const TILE_FILE_VERSION: u16 = 1;
/// Bytes before the error table.
const TILE_HEADER_BYTES: usize = 40;

/// One immutable tile: apron-padded world heights, bounds, and errors. Cloning shares the samples.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainTile {
    key: TileKey,
    revision: u64,

    // Lattice: `side * side` world heights, row-major from the apron corner, -Z row first.
    side: u32,
    heights: Arc<[f32]>,

    // Finest-surface bounds and the monotone error table.
    minimum: f64,
    maximum: f64,
    first_error_level: i32,
    errors: Arc<[f64]>,
}

impl TerrainTile {
    /// Validate and build a tile of `layout`.
    ///
    /// `heights` holds `stored_tile_side()^2` finite world heights including the apron. `errors` must be finite,
    /// nonnegative, monotone, start at zero, and cover every level from `first_error_level` to the tile's coarsest
    /// (`key.level + log2(tile_cells)`).
    pub fn new(
        layout: &TerrainLayout,
        key: TileKey,
        heights: Vec<f32>,
        minimum: f64,
        maximum: f64,
        first_error_level: i32,
        errors: Vec<f64>,
    ) -> Result<Self, TerrainError> {
        let side = layout.stored_tile_side();
        if !layout.contains(key, i32::MIN) {
            return Err(TerrainError::InvalidTile("the key lies outside the layout"));
        }
        if heights.len() != (side as usize) * (side as usize) || !heights.iter().all(|height| height.is_finite()) {
            return Err(TerrainError::InvalidTile("heights must hold the apron-padded lattice of finite values"));
        }
        if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
            return Err(TerrainError::InvalidTile("bounds must be finite and ordered"));
        }
        let expected_errors = key.level + layout.cells_log2() - first_error_level + 1;
        if first_error_level > key.level || errors.len() as i32 != expected_errors || !is_monotone_table(&errors) {
            return Err(TerrainError::InvalidTile("errors must be a monotone table from zero to the tile's coarsest"));
        }
        Ok(Self {
            key,
            revision: 0,
            side,
            heights: heights.into(),
            minimum,
            maximum,
            first_error_level,
            errors: errors.into(),
        })
    }

    /// This tile with another revision number; owners bump it whenever heights change.
    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    /// This tile with replaced heights, bounds, and errors (the same validation as [`TerrainTile::new`]).
    pub(crate) fn rebuilt(
        &self,
        layout: &TerrainLayout,
        heights: Vec<f32>,
        minimum: f64,
        maximum: f64,
        first_error_level: i32,
        errors: Vec<f64>,
    ) -> Result<Self, TerrainError> {
        Self::new(layout, self.key, heights, minimum, maximum, first_error_level, errors)
            .map(|tile| tile.with_revision(self.revision))
    }

    /// The tile's place in the pyramid.
    pub fn key(&self) -> TileKey {
        self.key
    }

    /// Owner-assigned revision; a renderer re-uploads a tile whose revision changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Samples per stored side, apron included.
    pub fn side(&self) -> u32 {
        self.side
    }

    /// Every stored height, apron included, row-major (-Z row first).
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// World height at lattice coordinate (`x`, `z`), where 0 is the tile's first interior sample and `tile_cells` its
    /// last; the apron extends one sample beyond both. Panics outside the stored lattice.
    pub fn height(&self, x: i32, z: i32) -> f32 {
        let apron = TILE_APRON as i32;
        let side = self.side as i32;
        let (column, row) = (x + apron, z + apron);
        assert!((0..side).contains(&column) && (0..side).contains(&row), "tile lattice coordinate outside the apron");
        self.heights[((row * side) + column) as usize]
    }

    /// Smallest finest-surface height inside the footprint, in world units.
    pub fn minimum(&self) -> f64 {
        self.minimum
    }

    /// Largest finest-surface height inside the footprint, in world units.
    pub fn maximum(&self) -> f64 {
        self.maximum
    }

    /// The global level of `errors()[0]`.
    pub fn first_error_level(&self) -> i32 {
        self.first_error_level
    }

    /// The monotone error table, finest level first.
    pub fn errors(&self) -> &[f64] {
        &self.errors
    }

    /// Bytes held by the samples and the error table.
    pub fn payload_bytes(&self) -> usize {
        std::mem::size_of_val(self.heights.as_ref()) + std::mem::size_of_val(self.errors.as_ref())
    }
}

/// Encode a stored tile (level zero or above) as a `.jtile` file, quantizing heights with the layout's range.
pub fn encode_tile(layout: &TerrainLayout, tile: &TerrainTile) -> Result<Vec<u8>, TerrainError> {
    let key = tile.key();
    if key.level < 0 || tile.first_error_level() != 0 {
        return Err(TerrainError::InvalidTile("only stored levels with errors from level zero are written"));
    }
    let mut bytes = Vec::with_capacity(TILE_HEADER_BYTES + (tile.errors().len() * 8) + (tile.heights().len() * 2));
    bytes.extend_from_slice(&TILE_FILE_MAGIC);
    bytes.extend_from_slice(&TILE_FILE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(layout.tile_cells as u16).to_le_bytes());
    bytes.extend_from_slice(&key.level.to_le_bytes());
    bytes.extend_from_slice(&key.x.to_le_bytes());
    bytes.extend_from_slice(&key.z.to_le_bytes());
    bytes.extend_from_slice(&(tile.errors().len() as u32).to_le_bytes());
    bytes.extend_from_slice(&tile.minimum().to_le_bytes());
    bytes.extend_from_slice(&tile.maximum().to_le_bytes());
    for error in tile.errors() {
        bytes.extend_from_slice(&error.to_le_bytes());
    }
    for height in tile.heights() {
        bytes.extend_from_slice(&layout.encode_height(f64::from(*height)).to_le_bytes());
    }
    Ok(bytes)
}

/// Decode a `.jtile` file of `layout`, checking its header against the layout and `expected` key.
pub fn decode_tile(layout: &TerrainLayout, expected: TileKey, bytes: &[u8]) -> Result<TerrainTile, TerrainError> {
    if bytes.len() < TILE_HEADER_BYTES || bytes[0..4] != TILE_FILE_MAGIC {
        return Err(TerrainError::InvalidTile("missing tile file header"));
    }
    let reader = LittleEndian(bytes);
    if reader.u16(4) != TILE_FILE_VERSION || u32::from(reader.u16(6)) != layout.tile_cells {
        return Err(TerrainError::InvalidTile("unsupported tile version or cell count"));
    }
    let key = TileKey::new(reader.i32(8), reader.u32(12), reader.u32(16));
    if key != expected {
        return Err(TerrainError::InvalidTile("the file holds a different tile"));
    }
    let error_count = reader.u32(20) as usize;
    let side = layout.stored_tile_side() as usize;
    let expected_length = TILE_HEADER_BYTES + (error_count * 8) + (side * side * 2);
    if bytes.len() != expected_length {
        return Err(TerrainError::InvalidTile("the payload length does not match the header"));
    }

    let minimum = reader.f64(24);
    let maximum = reader.f64(32);
    let errors = (0..error_count).map(|index| reader.f64(TILE_HEADER_BYTES + (index * 8))).collect();
    let samples_start = TILE_HEADER_BYTES + (error_count * 8);
    let heights =
        (0..side * side).map(|index| layout.decode_height(reader.u16(samples_start + (index * 2))) as f32).collect();
    TerrainTile::new(layout, key, heights, minimum, maximum, 0, errors)
}

/// Whether a table starts at zero and never decreases, with finite values.
fn is_monotone_table(errors: &[f64]) -> bool {
    errors.first() == Some(&0.0)
        && errors.iter().all(|error| error.is_finite() && *error >= 0.0)
        && errors.windows(2).all(|pair| pair[1] >= pair[0])
}

/// Little-endian reads at byte offsets already checked against the buffer length.
struct LittleEndian<'a>(&'a [u8]);

impl LittleEndian<'_> {
    fn u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.0[offset], self.0[offset + 1]])
    }

    fn u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.0[offset..offset + 4].try_into().expect("four bytes were length-checked"))
    }

    fn i32(&self, offset: usize) -> i32 {
        i32::from_le_bytes(self.0[offset..offset + 4].try_into().expect("four bytes were length-checked"))
    }

    fn f64(&self, offset: usize) -> f64 {
        f64::from_le_bytes(self.0[offset..offset + 8].try_into().expect("eight bytes were length-checked"))
    }
}
