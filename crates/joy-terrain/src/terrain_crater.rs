// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Craters: compact, additive height edits that every surface level, collider, and sample sees the same way.
//!
//! A crater is a smooth bowl with a raised lip, described by its center, radius, depth, and rim height. It is stored
//! as an operation, never as baked pixels: the edit log stays tiny to save or send over a network, applies to tiles
//! loaded later, and evaluates exactly at any lattice spacing. Offsets add, so craters commute and the surface after any
//! set of craters is independent of their order. Stored levels add the offset at their own samples (a coarse level
//! still equals the decimated fine level, because the edit is pointwise), and synthesized detail levels add it at
//! their finer lattice points, so craters stay crisp below the authored resolution.
//!
//! Positions and lengths are world units in the terrain's local space (before its instance transform).

use serde::{
    Deserialize,
    Serialize,
};

use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TILE_APRON,
    TerrainLayout,
};
use crate::terrain_tile::TerrainTile;

/// The rim reaches this many radii from the center; nothing beyond it changes.
pub const CRATER_REACH_RADII: f64 = 1.6;
/// The rim's crest sits on the bowl's edge.
const RIM_CENTER_RADII: f64 = 1.0;
/// Half the rim's width, in radii.
const RIM_HALF_WIDTH_RADII: f64 = 0.6;
/// Largest second derivatives of the bowl and rim profiles, per unit depth or height and radius squared.
const BOWL_CURVATURE: f64 = 8.0;
const RIM_CURVATURE: f64 = 8.0 / (RIM_HALF_WIDTH_RADII * RIM_HALF_WIDTH_RADII);
/// Largest accepted radius, depth, and rim height, in world units.
const MAXIMUM_CRATER_SIZE: f64 = 1.0e5;

/// One crater edit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainCrater {
    /// Center in local X/Z world units.
    pub center: [f64; 2],
    /// Bowl radius in world units; finite and positive.
    pub radius: f64,
    /// How far the bowl's center sinks, in world units; finite and nonnegative.
    pub depth: f64,
    /// How far the lip rises at the bowl's edge, in world units; finite and nonnegative.
    pub rim_height: f64,
}

impl TerrainCrater {
    /// Check every field; fails with the first invalid one.
    pub fn validate(&self) -> Result<(), TerrainError> {
        if !self.center.iter().all(|value| value.is_finite()) {
            return Err(TerrainError::InvalidCrater("center must be finite"));
        }
        if !self.radius.is_finite() || self.radius <= 0.0 || self.radius > MAXIMUM_CRATER_SIZE {
            return Err(TerrainError::InvalidCrater("radius must be finite, positive, and at most 100000"));
        }
        let in_range = |value: f64| value.is_finite() && (0.0..=MAXIMUM_CRATER_SIZE).contains(&value);
        if !in_range(self.depth) || !in_range(self.rim_height) {
            return Err(TerrainError::InvalidCrater("depth and rimHeight must be finite, nonnegative, and bounded"));
        }
        Ok(())
    }

    /// Distance from the center beyond which the crater changes nothing, in world units.
    pub fn reach(&self) -> f64 {
        self.radius * CRATER_REACH_RADII
    }

    /// The height this crater adds at local (`x`, `z`), in world units (negative inside the bowl).
    pub fn offset_at(&self, x: f64, z: f64) -> f64 {
        let (dx, dz) = (x - self.center[0], z - self.center[1]);
        let distance_squared = (dx * dx) + (dz * dz);
        let reach = self.reach();
        if distance_squared >= reach * reach {
            return 0.0;
        }
        let radii = distance_squared.sqrt() / self.radius;

        // A quartic bowl and a quartic rim bump: both smooth, both flat where they end.
        let mut offset = 0.0;
        if radii < 1.0 {
            let bowl = 1.0 - (radii * radii);
            offset -= self.depth * bowl * bowl;
        }
        let rim = (radii - RIM_CENTER_RADII) / RIM_HALF_WIDTH_RADII;
        if rim.abs() < 1.0 {
            let lip = 1.0 - (rim * rim);
            offset += self.rim_height * lip * lip;
        }
        offset
    }

    /// Whether the crater reaches into a local X/Z rectangle.
    pub fn touches(&self, minimum: [f64; 2], maximum: [f64; 2]) -> bool {
        let dx = (minimum[0] - self.center[0]).max(0.0).max(self.center[0] - maximum[0]);
        let dz = (minimum[1] - self.center[1]).max(0.0).max(self.center[1] - maximum[1]);
        ((dx * dx) + (dz * dz)) < (self.reach() * self.reach())
    }

    /// Bound on how far a triangle lattice with `spacing` world units misses this crater's exact profile.
    ///
    /// Linear interpolation misses a function by at most `spacing^2 / 8` times its second derivative; the doubled
    /// bound covers both triangle directions. Never more than the crater's whole height range.
    fn interpolation_error(&self, spacing: f64) -> f64 {
        let ratio = spacing / self.radius;
        let curvature = (BOWL_CURVATURE * self.depth) + (RIM_CURVATURE * self.rim_height);
        (0.25 * curvature * ratio * ratio).min(self.depth + self.rim_height)
    }
}

/// Add `craters` to a stored tile's lattice (apron included) and widen its bounds and errors to match.
///
/// `source` must be the tile as loaded, before any crater, and `craters` every crater touching its footprint. The
/// finest error entry stays zero: the edited lattice is the new finest surface of that level.
pub(crate) fn apply_craters(
    layout: &TerrainLayout,
    source: &TerrainTile,
    craters: &[TerrainCrater],
) -> Result<TerrainTile, TerrainError> {
    if craters.is_empty() {
        return Ok(source.clone());
    }
    let key = source.key();
    let origin = layout.tile_origin_samples(key);
    let step = joy_core::math::deterministic::exp2(f64::from(key.level));
    let side = layout.stored_tile_side() as i32;
    let apron = TILE_APRON as i32;

    let mut heights = source.heights().to_vec();
    for row in 0..side {
        for column in 0..side {
            let sample_x = origin[0] + (f64::from(column - apron) * step);
            let sample_z = origin[1] + (f64::from(row - apron) * step);
            let position = layout.sample_to_local(sample_x, sample_z);
            let offset: f64 = craters.iter().map(|crater| crater.offset_at(position[0], position[1])).sum();
            let index = ((row * side) + column) as usize;
            heights[index] = (f64::from(heights[index]) + offset) as f32;
        }
    }

    // Offsets lie in [-depth, rim height], so the finest surface stays inside the widened bounds.
    let lowered: f64 = craters.iter().map(|crater| crater.depth).sum();
    let raised: f64 = craters.iter().map(|crater| crater.rim_height).sum();
    let first = source.first_error_level();
    let errors: Vec<f64> = source
        .errors()
        .iter()
        .enumerate()
        .map(|(index, error)| {
            if index == 0 {
                return 0.0;
            }
            let spacing =
                joy_core::math::deterministic::exp2(f64::from(first + index as i32)) * layout.maximum_spacing();
            error + craters.iter().map(|crater| crater.interpolation_error(spacing)).sum::<f64>()
        })
        .collect();
    source.rebuilt(layout, heights, source.minimum() - lowered, source.maximum() + raised, first, errors)
}
