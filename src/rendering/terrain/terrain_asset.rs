// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Resident heightfield assets (format version 1): manifest validation, little-endian decoding, and the stable
//! finest-surface height sample used by collision and gameplay.
//!
//! The binary payload is row-major `u16` little endian. Source row zero maps to negative world Z and columns to positive
//! X; world X/Z are centered on the landscape. Scales are explicit world units, and camera LOD never changes this data.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::rendering::mesh::Bounds;

/// Supported manifest version.
const TERRAIN_FORMAT_VERSION: u32 = 1;
/// Supported raster orientation.
const TERRAIN_ORIENTATION: &str = "row-0-to-negative-z";
const MINIMUM_HEIGHTFIELD_SIZE: u32 = 3;
const MAXIMUM_HEIGHTFIELD_SIZE: u32 = 4097;
const MINIMUM_TILE_CELLS: u32 = 2;
const MAXIMUM_TILE_CELLS: u32 = 128;
const MAXIMUM_PATCH_COUNT: usize = 4096;
const BYTES_PER_HEIGHT_SAMPLE: usize = 2;
/// Largest quantized sample; samples decode as `offset + (sample / 65535) * scale`.
pub(crate) const MAXIMUM_QUANTIZED_HEIGHT: f64 = 65535.0;

/// Process-unique asset identities for GPU adapter caches.
static NEXT_TERRAIN_ID: AtomicU64 = AtomicU64::new(1);

/// Failure decoding terrain data or preparing a terrain frame.
#[derive(Debug, thiserror::Error)]
pub enum TerrainError {
    /// The manifest or height file could not be read.
    #[error("Terrain file could not be read: {0}")]
    Io(String),
    /// The manifest is not valid JSON for the version 1 schema.
    #[error("Invalid terrain manifest: {0}")]
    InvalidManifest(String),
    /// Dimensions, version, orientation, scales, or the binary length are invalid.
    #[error("Invalid terrain dimensions, orientation, scales or height payload.")]
    InvalidDimensions,
    /// The patch list is not a complete row-major leaf grid.
    #[error("Invalid terrain patch count.")]
    InvalidPatchCount,
    /// A patch is out of place, has invalid bounds, or a non-monotone error hierarchy.
    #[error("Invalid terrain patch bounds or monotone error hierarchy.")]
    InvalidPatch,
    /// A view vector or scalar is non-finite or out of range.
    #[error("Invalid terrain view.")]
    InvalidView,
    /// The view-projection matrix is not finite.
    #[error("Invalid terrain matrix.")]
    InvalidMatrix,
    /// The world transform is not a finite affine transform.
    #[error("Terrain requires a finite affine world transform.")]
    InvalidTransform,
    /// A scene terrain instance has an invalid tolerance or target radius.
    #[error("{0}")]
    InvalidInstance(&'static str),
    /// The device cannot hold this heightfield (for example, WebGL 2 texture limits).
    #[error("Terrain is unsupported on this device: {0}")]
    Unsupported(String),
}

/// One fixed quadtree leaf: sample-space origin, world-space height bounds, and per-level geometric errors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainPatch {
    /// Sample-space X origin (a multiple of the tile size).
    pub x: u32,
    /// Sample-space Z origin (a multiple of the tile size).
    pub z: u32,
    /// Minimum world-space height inside the patch.
    pub min: f64,
    /// Maximum world-space height inside the patch.
    pub max: f64,
    /// Maximum world-space triangle approximation error for each power-of-two stride; monotone, starting at zero.
    pub errors: Vec<f64>,
}

/// The version 1 terrain manifest. Unknown fields (source inspection, digests, the coarse hierarchy) are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainManifest {
    /// Format version; must be 1.
    pub version: u32,
    /// Samples per side; a power of two plus one.
    pub size: u32,
    /// Cells per leaf side; a power of two from 2 to 128.
    pub tile_cells: u32,
    /// World X and Z extents in world units.
    pub extent: Vec<f64>,
    /// World height of quantized sample zero.
    pub height_offset: f64,
    /// World height range spanned by the quantized samples.
    pub height_scale: f64,
    /// Height payload path, relative to the manifest.
    pub height_file: String,
    /// Raster orientation; must be `row-0-to-negative-z`.
    pub orientation: String,
    /// Row-major leaf patches.
    pub patches: Vec<TerrainPatch>,
}

/// A validated, immutable resident heightfield shared by renderers and scene instances. Cloning is cheap.
#[derive(Clone, Debug)]
pub struct TerrainAsset {
    inner: Arc<TerrainAssetInner>,
}

#[derive(Debug)]
struct TerrainAssetInner {
    id: u64,
    manifest: TerrainManifest,
    heights: Vec<u16>,
    tiles_per_axis: u32,
    max_lod: u32,
    bounds: Bounds,
}

impl TerrainAsset {
    /// Process-unique identity used to key GPU adapters.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// The validated manifest.
    pub fn manifest(&self) -> &TerrainManifest {
        &self.inner.manifest
    }

    /// Decoded quantized samples, row-major, `size * size` long.
    pub fn heights(&self) -> &[u16] {
        &self.inner.heights
    }

    /// Row-major leaf patches.
    pub fn patches(&self) -> &[TerrainPatch] {
        &self.inner.manifest.patches
    }

    /// Leaf patches per side.
    pub fn tiles_per_axis(&self) -> u32 {
        self.inner.tiles_per_axis
    }

    /// Coarsest LOD level (`log2(tile_cells)`).
    pub fn max_lod(&self) -> u32 {
        self.inner.max_lod
    }

    /// Local-space bounds: centered X/Z extents and the patch height range.
    pub fn bounds(&self) -> Bounds {
        self.inner.bounds
    }

    /// Whether two handles refer to the same asset.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// World-space height of one sample, decoded with the global offset and scale.
    pub(crate) fn decoded_height(&self, index: usize) -> f64 {
        let manifest = &self.inner.manifest;
        manifest.height_offset
            + ((f64::from(self.inner.heights[index]) / MAXIMUM_QUANTIZED_HEIGHT) * manifest.height_scale)
    }
}

/// Read a manifest and its height payload from disk. The height file resolves relative to the manifest.
///
/// Browsers cannot read files; fetch or embed both files there and call [`parse_terrain`].
pub fn load_terrain(manifest_path: impl AsRef<Path>) -> Result<TerrainAsset, TerrainError> {
    let manifest_path = manifest_path.as_ref();
    let text = std::fs::read_to_string(manifest_path).map_err(|error| TerrainError::Io(error.to_string()))?;
    let manifest: TerrainManifest =
        serde_json::from_str(&text).map_err(|error| TerrainError::InvalidManifest(error.to_string()))?;
    let height_path = manifest_path.parent().unwrap_or_else(|| Path::new(".")).join(&manifest.height_file);
    let binary = std::fs::read(height_path).map_err(|error| TerrainError::Io(error.to_string()))?;
    decode_terrain(manifest, &binary)
}

/// Parse a manifest from JSON text and decode its borrowed height payload.
pub fn parse_terrain(manifest_json: &str, binary: &[u8]) -> Result<TerrainAsset, TerrainError> {
    let manifest: TerrainManifest =
        serde_json::from_str(manifest_json).map_err(|error| TerrainError::InvalidManifest(error.to_string()))?;
    decode_terrain(manifest, binary)
}

/// Validate a manifest before any GPU allocation and decode portable little-endian samples.
///
/// The binary payload is borrowed and copied; the returned asset owns its samples and the manifest.
pub fn decode_terrain(manifest: TerrainManifest, binary: &[u8]) -> Result<TerrainAsset, TerrainError> {
    if !has_valid_terrain_dimensions(&manifest, binary.len()) {
        return Err(TerrainError::InvalidDimensions);
    }

    // Power-of-two cell counts make every coarser lattice land on authored samples and preserve patch corners all the
    // way to the coarsest level.
    let tiles_per_axis = (manifest.size - 1) / manifest.tile_cells;
    let max_lod = manifest.tile_cells.trailing_zeros();
    let expected_patches = (tiles_per_axis as usize) * (tiles_per_axis as usize);
    if manifest.patches.len() != expected_patches || manifest.patches.len() > MAXIMUM_PATCH_COUNT {
        return Err(TerrainError::InvalidPatchCount);
    }

    // Selection and neighbor lookup rely on a complete row-major leaf grid. Monotone errors let selection stop at the
    // first level over its budget.
    for (index, patch) in manifest.patches.iter().enumerate() {
        if !is_valid_terrain_patch(patch, index, tiles_per_axis, manifest.tile_cells, max_lod) {
            return Err(TerrainError::InvalidPatch);
        }
    }

    // Read byte order explicitly rather than depending on the host's endianness.
    let heights: Vec<u16> =
        binary.as_chunks::<BYTES_PER_HEIGHT_SAMPLE>().0.iter().map(|bytes| u16::from_le_bytes(*bytes)).collect();

    let minimum = manifest.patches.iter().map(|patch| patch.min).fold(f64::INFINITY, f64::min);
    let maximum = manifest.patches.iter().map(|patch| patch.max).fold(f64::NEG_INFINITY, f64::max);
    let half_x = (manifest.extent[0] * 0.5) as f32;
    let half_z = (manifest.extent[1] * 0.5) as f32;
    let bounds =
        Bounds { min: Vec3::new(-half_x, minimum as f32, -half_z), max: Vec3::new(half_x, maximum as f32, half_z) };

    let inner = TerrainAssetInner {
        id: NEXT_TERRAIN_ID.fetch_add(1, Ordering::Relaxed),
        manifest,
        heights,
        tiles_per_axis,
        max_lod,
        bounds,
    };
    Ok(TerrainAsset { inner: Arc::new(inner) })
}

/// Stable collision and gameplay height, independent of camera and render topology.
///
/// Coordinates are world units, clamped to the landscape bounds. Samples the finest resident surface with the same
/// diagonal triangle split used for rendering.
pub fn sample_terrain(asset: &TerrainAsset, world_x: f64, world_z: f64) -> f64 {
    let manifest = asset.manifest();
    let cells = f64::from(manifest.size - 1);

    // World X/Z are centered on the landscape; sample coordinates start at zero.
    let sample_x = ((world_x / manifest.extent[0]) + 0.5) * cells;
    let sample_z = ((world_z / manifest.extent[1]) + 0.5) * cells;
    sample_height_triangle(asset, sample_x, sample_z, 1)
}

/// Sample the authored triangle lattice at a positive power-of-two `stride`, in heightfield sample coordinates.
///
/// Returns the world-space height on the rendered triangle surface (not a bilinear blend).
pub fn sample_height_triangle(asset: &TerrainAsset, x: f64, z: f64, stride: u32) -> f64 {
    let manifest = asset.manifest();
    let size = manifest.size as usize;
    let last = f64::from(manifest.size - 1);
    let stride_value = f64::from(stride);
    let clamped_x = x.clamp(0.0, last);
    let clamped_z = z.clamp(0.0, last);

    // At the positive boundary, use the final complete cell with fraction one rather than a cell beginning outside the
    // heightfield.
    let base_limit = last - stride_value;
    let base_x = base_limit.min((clamped_x / stride_value).floor() * stride_value);
    let base_z = base_limit.min((clamped_z / stride_value).floor() * stride_value);
    let fraction_x = (clamped_x - base_x) / stride_value;
    let fraction_z = (clamped_z - base_z) / stride_value;

    let (column, row, step) = (base_x as usize, base_z as usize, stride as usize);
    let heights = asset.heights();
    let top_left = f64::from(heights[(row * size) + column]);
    let top_right = f64::from(heights[(row * size) + column + step]);
    let bottom_left = f64::from(heights[((row + step) * size) + column]);
    let bottom_right = f64::from(heights[((row + step) * size) + column + step]);

    // The top-left to bottom-right diagonal divides the cell into two planes. Bilinear filtering would produce a
    // different surface from the triangles used for rendering.
    let quantized = if fraction_x >= fraction_z {
        top_left + ((top_right - top_left) * fraction_x) + ((bottom_right - top_right) * fraction_z)
    } else {
        top_left + ((bottom_right - bottom_left) * fraction_x) + ((bottom_left - top_left) * fraction_z)
    };
    manifest.height_offset + ((quantized / MAXIMUM_QUANTIZED_HEIGHT) * manifest.height_scale)
}

/// Validate manifest-wide dimensions and scalar terrain metadata.
fn has_valid_terrain_dimensions(manifest: &TerrainManifest, binary_length: usize) -> bool {
    let size = manifest.size;
    let tile_cells = manifest.tile_cells;
    if !(MINIMUM_HEIGHTFIELD_SIZE..=MAXIMUM_HEIGHTFIELD_SIZE).contains(&size)
        || !(MINIMUM_TILE_CELLS..=MAXIMUM_TILE_CELLS).contains(&tile_cells)
    {
        return false;
    }
    let heightfield_cells = size - 1;
    let has_valid_extent =
        manifest.extent.len() == 2 && manifest.extent.iter().all(|value| value.is_finite() && *value > 0.0);
    let expected_length = (size as usize) * (size as usize) * BYTES_PER_HEIGHT_SAMPLE;

    manifest.version == TERRAIN_FORMAT_VERSION
        && manifest.orientation == TERRAIN_ORIENTATION
        && heightfield_cells.is_power_of_two()
        && tile_cells.is_power_of_two()
        && heightfield_cells.is_multiple_of(tile_cells)
        && has_valid_extent
        && manifest.height_offset.is_finite()
        && manifest.height_scale.is_finite()
        && manifest.height_scale >= 0.0
        && binary_length == expected_length
}

/// Validate one patch's row-major position, bounds, and monotone error hierarchy.
fn is_valid_terrain_patch(
    patch: &TerrainPatch,
    index: usize,
    tiles_per_axis: u32,
    tile_cells: u32,
    max_lod: u32,
) -> bool {
    let tiles = tiles_per_axis as usize;
    let expected_x = ((index % tiles) as u32) * tile_cells;
    let expected_z = ((index / tiles) as u32) * tile_cells;
    let has_valid_bounds = patch.min.is_finite() && patch.max.is_finite() && patch.min <= patch.max;
    let has_valid_errors = patch.errors.len() == (max_lod as usize) + 1
        && patch.errors[0] == 0.0
        && patch.errors.iter().enumerate().all(|(level, error)| {
            let previous = if level > 0 { patch.errors[level - 1] } else { 0.0 };
            error.is_finite() && *error >= 0.0 && *error >= previous
        });

    patch.x == expected_x && patch.z == expected_z && has_valid_bounds && has_valid_errors
}
