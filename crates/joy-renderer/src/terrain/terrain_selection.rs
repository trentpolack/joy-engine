// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! CPU leaf selection and the morphed reference surface.
//!
//! Each fixed quadtree leaf picks a fractional LOD from its prebuilt geometric errors and the conservative distance to
//! its bounds; neighbors then share the coarser fractional level along each edge. The compute shader
//! (`terrain-select.wgsl`) applies the same policy on WebGPU-class backends. Math runs in `f64` with V8's `Math.hypot`
//! algorithm like the TypeScript engine, so CPU results match it bit for bit; GPU uploads convert to `f32`.

use joy_core::math::DVec3;

use crate::terrain::terrain_asset::{
    TerrainAsset,
    TerrainError,
    TerrainPatch,
    sample_height_triangle,
};
use joy_core::math::deterministic;

/// Smallest camera distance used for the error budget, in world units.
const MINIMUM_DISTANCE: f64 = 0.01;
/// Largest projected cell size in CSS pixels, independent of geometric roughness.
pub(crate) const MAXIMUM_CELL_PIXELS: f64 = 32.0;

/// Camera and protected-target state for selection. Positions are world units (local to the terrain when it is
/// transformed); `projection_scale` is the CSS viewport height divided by `2 * tan(vertical FOV / 2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainView {
    /// Eye position.
    pub eye: DVec3,
    /// CSS pixels per world unit at unit distance; positive.
    pub projection_scale: f64,
    /// Screen-space error tolerance in CSS pixels; positive.
    pub tolerance: f64,
    /// Protected target position; leaves near it keep the finest detail.
    pub target: DVec3,
    /// Horizontal radius around the target pinned to finest detail, fading over one more radius; zero disables it.
    pub target_radius: f64,
}

/// One leaf's selection: fractional detail, the integer LOD that picks its index range, and reconciled edge details.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSelection {
    /// Fractional LOD; the fraction morphs heights toward the next coarser lattice.
    pub detail: f64,
    /// `floor(detail)`, the index range drawn this frame.
    pub lod: usize,
    /// Edge details ordered -X, +X, -Z, +Z (the coarser of this leaf and its neighbor).
    pub edges: [f64; 4],
}

/// Validate camera and target inputs before any selection or upload.
pub fn validate_terrain_view(view: &TerrainView) -> Result<(), TerrainError> {
    let valid = view.eye.is_finite()
        && view.target.is_finite()
        && view.projection_scale.is_finite()
        && view.projection_scale > 0.0
        && view.tolerance.is_finite()
        && view.tolerance > 0.0
        && view.target_radius.is_finite()
        && view.target_radius >= 0.0;
    if valid { Ok(()) } else { Err(TerrainError::InvalidView) }
}

/// Select every leaf's fractional LOD with conservative bounds distance and prebuilt errors, then reconcile edges.
///
/// Projected cells are limited to 32 CSS pixels. Target distance is horizontal. No heights are scanned or mutated.
pub fn select_terrain(asset: &TerrainAsset, view: &TerrainView) -> Result<Vec<TerrainSelection>, TerrainError> {
    validate_terrain_view(view)?;
    let manifest = asset.manifest();
    let sample_spacing = manifest.extent[0].max(manifest.extent[1]) / f64::from(manifest.size - 1);
    let details: Vec<f64> =
        asset.patches().iter().map(|patch| calculate_patch_detail(asset, patch, view, sample_spacing)).collect();
    reconcile_terrain_edges(asset, &details)
}

/// Reconcile each leaf with its neighbors' fractional details; `details` is in row-major patch order.
///
/// Edge endpoints are authored tile corners at every level, so four-way corners agree. Fails when `details` does not
/// have one entry per patch.
pub fn reconcile_terrain_edges(asset: &TerrainAsset, details: &[f64]) -> Result<Vec<TerrainSelection>, TerrainError> {
    if details.len() != asset.patches().len() {
        return Err(TerrainError::InvalidPatchCount);
    }
    let axis = asset.tiles_per_axis() as usize;
    let selections = details
        .iter()
        .enumerate()
        .map(|(index, &detail)| {
            let patch_x = index % axis;
            let patch_z = index / axis;
            let negative_x = if patch_x > 0 { details[index - 1] } else { detail };
            let positive_x = if patch_x < axis - 1 { details[index + 1] } else { detail };
            let negative_z = if patch_z > 0 { details[index - axis] } else { detail };
            let positive_z = if patch_z < axis - 1 { details[index + axis] } else { detail };
            TerrainSelection {
                detail,
                lod: detail.floor() as usize,
                edges: [detail.max(negative_x), detail.max(positive_x), detail.max(negative_z), detail.max(positive_z)],
            }
        })
        .collect();
    Ok(selections)
}

/// Sample the CPU reference of the continuously morphed surface at patch-local sample coordinates.
///
/// This is the surface the vertex shader draws, used for seam and shader parity checks. Returns `None` for an
/// out-of-range patch index.
pub fn sample_morphed_height(
    asset: &TerrainAsset,
    patch_index: usize,
    x: f64,
    z: f64,
    selection: &TerrainSelection,
) -> Option<f64> {
    let patch = asset.patches().get(patch_index)?;
    let cells = f64::from(asset.manifest().tile_cells);
    let edge_detail = edge_detail(x, z, cells, selection.detail, selection);

    if x == 0.0 || x == cells || z == 0.0 || z == cells {
        return Some(sample_detail_height(asset, patch, cells, x, z, edge_detail));
    }

    let level = selection.detail.floor();
    let fine_stride = deterministic::exp2(level);
    let coarse_stride = cells.min(fine_stride * 2.0);
    let fine = sample_morphed_surface(asset, patch, cells, x, z, fine_stride, selection);
    let coarse = sample_morphed_surface(asset, patch, cells, x, z, coarse_stride, selection);
    Some(fine + ((coarse - fine) * (selection.detail - level)))
}

/// One leaf's fractional detail from its screen-space error budget.
fn calculate_patch_detail(asset: &TerrainAsset, patch: &TerrainPatch, view: &TerrainView, sample_spacing: f64) -> f64 {
    let manifest = asset.manifest();
    let cells = f64::from(manifest.size - 1);
    let tile = f64::from(manifest.tile_cells);
    let (extent_x, extent_z) = (manifest.extent[0], manifest.extent[1]);
    let minimum = DVec3::new(
        ((f64::from(patch.x) / cells) * extent_x) - (extent_x / 2.0),
        patch.min,
        ((f64::from(patch.z) / cells) * extent_z) - (extent_z / 2.0),
    );
    let maximum =
        DVec3::new(minimum.x + ((tile / cells) * extent_x), patch.max, minimum.z + ((tile / cells) * extent_z));

    // Pin finest detail where the horizontal footprint touches the target disk.
    let target_x = (minimum.x - view.target.x).max(0.0).max(view.target.x - maximum.x);
    let target_z = (minimum.z - view.target.z).max(0.0).max(view.target.z - maximum.z);
    let target_distance = deterministic::hypot(target_x, target_z);
    if view.target_radius > 0.0 && target_distance < view.target_radius {
        return 0.0;
    }

    // Pixel tolerance becomes a world-height error budget at the nearest distance to the bounds.
    let offsets = (minimum - view.eye).max(DVec3::ZERO).max(view.eye - maximum);
    let camera_distance = MINIMUM_DISTANCE.max(offsets.length());
    let budget = (camera_distance / view.projection_scale) * view.tolerance;
    let errors: Vec<f64> = patch
        .errors
        .iter()
        .enumerate()
        .map(|(level, error)| {
            let cell_error = if level == 0 {
                0.0
            } else {
                ((deterministic::powi(2.0_f64, level as i32) * sample_spacing) * view.tolerance) / MAXIMUM_CELL_PIXELS
            };
            error.max(cell_error)
        })
        .collect();

    let max_lod = asset.max_lod() as usize;
    let mut level = 0;
    while level < max_lod && errors[level + 1] <= budget {
        level += 1;
    }
    let detail = fractional_detail(level, max_lod, budget, &errors);

    // Fade the pin over one more radius with smoothstep, avoiding a hard transition at its boundary.
    let fade = if view.target_radius > 0.0 {
        ((target_distance - view.target_radius) / view.target_radius).clamp(0.0, 1.0)
    } else {
        1.0
    };
    detail * (fade * fade * (3.0 - (2.0 * fade)))
}

/// Continuous detail between a selected lattice and its parent.
fn fractional_detail(level: usize, max_lod: usize, budget: f64, errors: &[f64]) -> f64 {
    if level == max_lod {
        return level as f64;
    }
    let range = errors[level + 1] - errors[level];
    level as f64 + ((budget - errors[level]) / range).clamp(0.0, 1.0)
}

/// Boundary detail from the reconciled neighbor edges; interior points keep `detail`.
fn edge_detail(x: f64, z: f64, cells: f64, detail: f64, selection: &TerrainSelection) -> f64 {
    let mut result = detail;
    if x == 0.0 {
        result = selection.edges[0];
    }
    if x == cells {
        result = selection.edges[1];
    }
    if z == 0.0 {
        result = result.max(selection.edges[2]);
    }
    if z == cells {
        result = result.max(selection.edges[3]);
    }
    result
}

/// Blend the two authored triangle lattices surrounding a fractional detail.
fn sample_detail_height(asset: &TerrainAsset, patch: &TerrainPatch, cells: f64, x: f64, z: f64, detail: f64) -> f64 {
    let level = detail.floor();
    let fine_stride = deterministic::exp2(level);
    let coarse_stride = cells.min(fine_stride * 2.0);
    let sample_x = f64::from(patch.x) + x;
    let sample_z = f64::from(patch.z) + z;
    let fine = sample_height_triangle(asset, sample_x, sample_z, fine_stride as u32);
    let coarse = sample_height_triangle(asset, sample_x, sample_z, coarse_stride as u32);
    fine + ((coarse - fine) * (detail - level))
}

/// Reconstruct the stitched triangle containing one patch-local coordinate at `stride`.
fn sample_morphed_surface(
    asset: &TerrainAsset,
    patch: &TerrainPatch,
    cells: f64,
    x: f64,
    z: f64,
    stride: f64,
    selection: &TerrainSelection,
) -> f64 {
    let base_x = ((x / stride).floor() * stride).min(cells - stride);
    let base_z = ((z / stride).floor() * stride).min(cells - stride);
    let fraction_x = (x - base_x) / stride;
    let fraction_z = (z - base_z) / stride;
    let vertex = |vertex_x: f64, vertex_z: f64| {
        sample_stitched_vertex(asset, patch, cells, vertex_x, vertex_z, stride, selection)
    };
    let top_left = vertex(base_x, base_z);
    let top_right = vertex(base_x + stride, base_z);
    let bottom_left = vertex(base_x, base_z + stride);
    let bottom_right = vertex(base_x + stride, base_z + stride);

    if fraction_x >= fraction_z {
        top_left + ((top_right - top_left) * fraction_x) + ((bottom_right - top_right) * fraction_z)
    } else {
        top_left + ((bottom_right - bottom_left) * fraction_x) + ((bottom_left - top_left) * fraction_z)
    }
}

/// A parent vertex after applying any coarser neighboring edge.
fn sample_stitched_vertex(
    asset: &TerrainAsset,
    patch: &TerrainPatch,
    cells: f64,
    x: f64,
    z: f64,
    stride: f64,
    selection: &TerrainSelection,
) -> f64 {
    let detail = edge_detail(x, z, cells, deterministic::log2(stride), selection);
    sample_detail_height(asset, patch, cells, x, z, detail)
}
