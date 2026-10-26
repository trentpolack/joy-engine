// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Synthesized detail below the authored resolution, shaped by the surface's material.
//!
//! Detail adds pyramid levels below zero. A level `-k` tile halves the lattice spacing `k` times and samples one
//! continuous surface: a Catmull-Rom reconstruction of the authored level-0 samples (smooth where the authored
//! triangles are faceted) plus band-limited gradient-noise octaves whose amplitude and character follow the material
//! under them (rock is rough and ridged, ground is gentle, snow is nearly smooth). Craters then add their exact
//! profiles. Levels zero and above keep their authored samples, so detail fades in only where the camera or a collider
//! needs finer geometry, and the surface between levels still morphs continuously.
//!
//! The noise is a residual: each octave minus its own Catmull-Rom interpolation from the level-0 lattice points. Detail
//! therefore adds only what the authored lattice cannot draw and vanishes exactly on that lattice, so every level
//! agrees at every shared lattice point. A level-0 tile beside its synthesized children shares their edge samples (no
//! cracks), a child drawn at its parent's lattice is the parent (no pops), and normals built from neighboring lattice
//! points of any level agree.
//!
//! Every value is computed on the CPU in `f64` from integer hashes and arithmetic alone, so a synthesized tile is
//! identical on every target and the renderer, collision, and gameplay sampling all read the same samples. Detail is
//! deterministic gameplay data: its seed belongs to the terrain, never to a cosmetic random stream.
//!
//! Material weights come from slope and height ([`TerrainMaterialRules`]); the renderer colors terrain with the same
//! rules, so the roughest detail appears where rock is drawn.

use std::f64::consts::PI;

use joy_core::math::deterministic;
use serde::{
    Deserialize,
    Serialize,
};

use crate::terrain_crater::TerrainCrater;
use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TILE_APRON,
    TerrainLayout,
    TileKey,
};
use crate::terrain_tile::TerrainTile;

/// Most synthesized levels below the authored lattice (a sixteenth of the authored spacing).
pub const MAXIMUM_DETAIL_LEVELS: u32 = 4;
/// Most noise octaves.
pub const MAXIMUM_DETAIL_OCTAVES: u32 = 8;

/// Gradient noise peaks a little above one; this bounds its magnitude for error and bounds estimates.
const NOISE_PEAK: f64 = 1.1;
/// The residual noise is the noise minus a two-dimensional Catmull-Rom interpolation of it, whose weights sum in
/// magnitude to at most 1.25 per axis; the residual is therefore at most `1 + 1.25^2` times the noise's peak.
const RESIDUAL_PEAK: f64 = 1.0 + (1.25 * 1.25);

/// How slope and height choose the surface material. The renderer's terrain colors and detail synthesis share them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainMaterialRules {
    /// Slope (`1 - |normal.y|`) where rock starts to replace ground, and where it has fully replaced it.
    pub rock_slope: [f64; 2],
    /// Height in world units where snow starts to cover the surface, and where it fully covers it.
    pub snow_height: [f64; 2],
}

impl Default for TerrainMaterialRules {
    fn default() -> Self {
        // The thresholds the terrain shader has always used.
        Self {
            rock_slope: [0.1, 0.65],
            snow_height: [390.0, 520.0],
        }
    }
}

impl TerrainMaterialRules {
    /// Check that both ranges are finite and ascending.
    pub fn validate(&self) -> Result<(), TerrainError> {
        let ascending = |range: [f64; 2]| range.iter().all(|value| value.is_finite()) && range[0] < range[1];
        if !ascending(self.rock_slope) || !ascending(self.snow_height) {
            return Err(TerrainError::InvalidDetail("material ranges must be finite and ascending"));
        }
        Ok(())
    }

    /// Ground, rock, and snow weights (summing to one) for a surface normal's Y component and a world height.
    pub fn weights(&self, normal_y: f64, height: f64) -> TerrainMaterialWeights {
        let rock = smoothstep(self.rock_slope[0], self.rock_slope[1], 1.0 - normal_y.abs());
        let snow = smoothstep(self.snow_height[0], self.snow_height[1], height);
        TerrainMaterialWeights {
            ground: (1.0 - rock) * (1.0 - snow),
            rock: rock * (1.0 - snow),
            snow,
        }
    }
}

/// Blend weights of the three terrain materials; they sum to one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainMaterialWeights {
    /// Gentle ground (grass and soil).
    pub ground: f64,
    /// Steep rock.
    pub rock: f64,
    /// High snow.
    pub snow: f64,
}

/// One material's detail character.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainDetailMaterial {
    /// Amplitude of the first (longest) octave in world units; finite and nonnegative.
    pub amplitude: f64,
    /// Blend from smooth rolling noise (0) to sharp ridged noise (1).
    pub ridged: f64,
}

/// Detail synthesis settings for a streamed surface. Plain data; validate with [`TerrainDetail::validate`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainDetail {
    /// Synthesized levels below the authored lattice, 1 to 4; each halves the spacing again.
    pub levels: u32,
    /// The level gameplay sampling and terrain colliders read, from 0 (authored triangles) down to `-levels`.
    pub collision_level: i32,
    /// Wavelength of the first octave in authored samples; a power of two from 1 to 64.
    pub base_wavelength_samples: u32,
    /// Octaves, 1 to 8; each halves the wavelength of the one before it.
    pub octaves: u32,
    /// Amplitude ratio between consecutive octaves, in `(0, 1]`.
    pub persistence: f64,
    /// Character on gentle slopes.
    pub ground: TerrainDetailMaterial,
    /// Character on steep slopes.
    pub rock: TerrainDetailMaterial,
    /// Character on high ground.
    pub snow: TerrainDetailMaterial,
    /// Selects the noise pattern; part of the terrain's gameplay data.
    pub seed: u32,
}

impl Default for TerrainDetail {
    fn default() -> Self {
        Self {
            levels: 2,
            collision_level: -2,
            base_wavelength_samples: 4,
            octaves: 3,
            persistence: 0.5,
            ground: TerrainDetailMaterial {
                amplitude: 0.3,
                ridged: 0.0,
            },
            rock: TerrainDetailMaterial {
                amplitude: 1.2,
                ridged: 0.8,
            },
            snow: TerrainDetailMaterial {
                amplitude: 0.12,
                ridged: 0.0,
            },
            seed: 0x7E44_A1D5,
        }
    }
}

impl TerrainDetail {
    /// Check every field; fails with the first invalid one.
    pub fn validate(&self) -> Result<(), TerrainError> {
        if !(1..=MAXIMUM_DETAIL_LEVELS).contains(&self.levels) {
            return Err(TerrainError::InvalidDetail("levels must be between 1 and 4"));
        }
        if self.collision_level > 0 || self.collision_level < -(self.levels as i32) {
            return Err(TerrainError::InvalidDetail("collisionLevel must be between -levels and 0"));
        }
        if !self.base_wavelength_samples.is_power_of_two() || self.base_wavelength_samples > 64 {
            return Err(TerrainError::InvalidDetail("baseWavelengthSamples must be a power of two from 1 to 64"));
        }
        if !(1..=MAXIMUM_DETAIL_OCTAVES).contains(&self.octaves) {
            return Err(TerrainError::InvalidDetail("octaves must be between 1 and 8"));
        }
        if !self.persistence.is_finite() || self.persistence <= 0.0 || self.persistence > 1.0 {
            return Err(TerrainError::InvalidDetail("persistence must be in (0, 1]"));
        }
        for material in [self.ground, self.rock, self.snow] {
            let valid_amplitude = material.amplitude.is_finite() && material.amplitude >= 0.0;
            if !valid_amplitude || !(0.0..=1.0).contains(&material.ridged) {
                return Err(TerrainError::InvalidDetail("material amplitudes must be nonnegative, ridged in [0, 1]"));
            }
        }
        Ok(())
    }

    /// The finest synthesized level (`-levels`).
    pub fn finest_level(&self) -> i32 {
        -(self.levels as i32)
    }

    /// Largest height the detail can add or remove anywhere, in world units.
    pub fn amplitude_bound(&self) -> f64 {
        let largest = self.ground.amplitude.max(self.rock.amplitude).max(self.snow.amplitude);
        let octaves: f64 = (0..self.octaves).map(|octave| self.octave_scale(octave)).sum();
        largest * octaves * NOISE_PEAK * RESIDUAL_PEAK
    }

    /// The noise part of the error of drawing the lattice at `level` instead of the finest synthesized lattice, where
    /// the first octave's amplitude is at most `amplitude` world units.
    ///
    /// A sinusoid of amplitude `A` and wavelength `λ` drawn by linear interpolation at spacing `s` is off by at most
    /// `A (1 - cos(π s / λ))`, about `A (π s / λ)^2 / 2`, and never more than `2A`. Each octave only contributes the
    /// part its level-0 interpolation leaves behind ([`lattice_residual`]). Levels at or above zero miss every octave.
    pub(crate) fn noise_error(&self, level: i32, amplitude: f64) -> f64 {
        let largest = amplitude * NOISE_PEAK;
        let spacing = deterministic::exp2(f64::from(level.min(0)));
        let finest = deterministic::exp2(f64::from(self.finest_level()));
        (0..self.octaves)
            .map(|octave| {
                let wavelength = self.octave_wavelength(octave);
                let error = |stride: f64| (0.5 * deterministic::powi(PI * stride / wavelength, 2)).min(2.0);
                let residual = lattice_residual(wavelength);
                largest * self.octave_scale(octave) * residual * (error(spacing) - error(finest)).max(0.0)
            })
            .sum()
    }

    /// The largest first-octave amplitude any material under a tile's lattice uses, in world units.
    ///
    /// Materials come from the lattice's own slopes and heights, so flat ground far from rock refines into detail only
    /// as far as its gentle noise needs: synthesized detail costs follow the material, like material-driven
    /// tessellation.
    pub(crate) fn regional_amplitude(
        &self,
        rules: &TerrainMaterialRules,
        layout: &TerrainLayout,
        tile: &TerrainTile,
    ) -> f64 {
        let step = deterministic::exp2(f64::from(tile.key().level));
        let (spacing_x, spacing_z) = (layout.spacing[0] * step, layout.spacing[1] * step);
        let cells = layout.tile_cells as i32;
        let mut largest: f64 = 0.0;
        for z in 0..=cells {
            for x in 0..=cells {
                let slope_x = (f64::from(tile.height(x + 1, z)) - f64::from(tile.height(x - 1, z))) / (2.0 * spacing_x);
                let slope_z = (f64::from(tile.height(x, z + 1)) - f64::from(tile.height(x, z - 1))) / (2.0 * spacing_z);
                let normal_y = 1.0 / (1.0 + (slope_x * slope_x) + (slope_z * slope_z)).sqrt();
                let weights = rules.weights(normal_y, f64::from(tile.height(x, z)));
                let amplitude = (weights.ground * self.ground.amplitude)
                    + (weights.rock * self.rock.amplitude)
                    + (weights.snow * self.snow.amplitude);
                largest = largest.max(amplitude);
            }
        }
        largest
    }

    /// The detail height (noise only, without the reconstructed base) at a finest-lattice coordinate.
    ///
    /// `weights` blend the materials' amplitudes and ridged character.
    pub fn noise_height(&self, sample_x: f64, sample_z: f64, weights: TerrainMaterialWeights) -> f64 {
        let amplitude = (weights.ground * self.ground.amplitude)
            + (weights.rock * self.rock.amplitude)
            + (weights.snow * self.snow.amplitude);
        if amplitude <= 0.0 {
            return 0.0;
        }
        let ridged = (weights.ground * self.ground.ridged)
            + (weights.rock * self.rock.ridged)
            + (weights.snow * self.snow.ridged);

        let mut height = 0.0;
        for octave in 0..self.octaves {
            let wavelength = self.octave_wavelength(octave);
            let seed = self.seed.wrapping_add(octave.wrapping_mul(0x9E37_79B9));
            let smooth = gradient_noise(sample_x / wavelength, sample_z / wavelength, seed);

            // Ridged noise folds the smooth noise into sharp crests, recentered on zero.
            let crest = 1.0 - (2.0 * smooth.abs());
            let value = smooth + ((crest - smooth) * ridged);
            height += value * self.octave_scale(octave);
        }
        height * amplitude
    }

    /// Wavelength of one octave in finest samples.
    fn octave_wavelength(&self, octave: u32) -> f64 {
        f64::from(self.base_wavelength_samples) / f64::from(1_u32 << octave)
    }

    /// Amplitude multiplier of one octave (`persistence^octave`).
    fn octave_scale(&self, octave: u32) -> f64 {
        deterministic::powi(self.persistence, octave as i32)
    }
}

/// Synthesize the detail tile `key` (a negative level) from its level-0 ancestor.
///
/// `source` is the ancestor's authored lattice before any crater; `craters` are added exactly at the new lattice
/// points. `level_zero_errors` is the ancestor's effective table from the finest synthesized level up to its coarsest
/// (see [`effective_errors`]); the new tile keeps the prefix it can draw.
pub(crate) fn synthesize_tile(
    layout: &TerrainLayout,
    detail: &TerrainDetail,
    rules: &TerrainMaterialRules,
    key: TileKey,
    source: &TerrainTile,
    craters: &[TerrainCrater],
    level_zero_errors: &[f64],
) -> Result<TerrainTile, TerrainError> {
    let parent_origin = layout.tile_origin_samples(source.key());
    let origin = layout.tile_origin_samples(key);
    let step = deterministic::exp2(f64::from(key.level));
    let apron = TILE_APRON as i32;
    let cells = layout.tile_cells as i32;

    // The noise at the level-0 lattice points under the tile, which the residual subtracts.
    let first_local = [origin[0] - parent_origin[0] - step, origin[1] - parent_origin[1] - step];
    let span = f64::from(cells + (2 * apron)) * step;
    let lattice = LatticeNoise::new(detail, rules, layout, source, first_local, span);

    let mut heights = Vec::with_capacity((layout.stored_tile_side() as usize).pow(2));
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for row in -apron..=(cells + apron) {
        for column in -apron..=(cells + apron) {
            // Coordinates inside the level-0 tile; global coordinates keep the noise seamless across tiles.
            let local_x = (origin[0] - parent_origin[0]) + (f64::from(column) * step);
            let local_z = (origin[1] - parent_origin[1]) + (f64::from(row) * step);
            let sample_x = parent_origin[0] + local_x;
            let sample_z = parent_origin[1] + local_z;
            let base = reconstruct(source, layout, local_x, local_z);
            let normal_y = 1.0 / (1.0 + (base.slope_x * base.slope_x) + (base.slope_z * base.slope_z)).sqrt();
            let weights = rules.weights(normal_y, base.height);
            let position = layout.sample_to_local(sample_x, sample_z);
            let crater_height: f64 = craters.iter().map(|crater| crater.offset_at(position[0], position[1])).sum();
            let residual = detail.noise_height(sample_x, sample_z, weights) - lattice.interpolate(local_x, local_z);
            let height = base.height + residual + crater_height;

            let interior = (0..=cells).contains(&column) && (0..=cells).contains(&row);
            if interior {
                minimum = minimum.min(height);
                maximum = maximum.max(height);
            }
            heights.push(height as f32);
        }
    }

    // Lattice points sample the continuous surface, which can dip between them by about the noise error of one step.
    let margin = detail.noise_error(key.level + 1, detail.regional_amplitude(rules, layout, source));
    let shared = (key.level + layout.cells_log2() - detail.finest_level() + 1) as usize;
    let errors = level_zero_errors[..shared.min(level_zero_errors.len())].to_vec();
    TerrainTile::new(layout, key, heights, minimum - margin, maximum + margin, detail.finest_level(), errors)
}

/// A level-0 tile's error table extended down to the finest synthesized level and measured against the synthesized
/// surface: detail levels carry the noise error of the tile's materials, and authored levels add the level-0 offset.
pub(crate) fn effective_errors(
    detail: &TerrainDetail,
    rules: &TerrainMaterialRules,
    layout: &TerrainLayout,
    source: &TerrainTile,
) -> Vec<f64> {
    // Only the noise drives refinement: the smooth reconstruction blends in continuously as detail levels draw, and
    // counting its difference from the authored triangles would refine every cliff into detail at any distance.
    let amplitude = detail.regional_amplitude(rules, layout, source);
    let detail_error = |level: i32| detail.noise_error(level, amplitude);
    let mut errors: Vec<f64> = (detail.finest_level()..0).map(detail_error).collect();
    errors[0] = 0.0;
    let offset = detail_error(0);
    errors.extend(source.errors().iter().map(|error| error + offset));
    make_monotone(&mut errors);
    errors
}

/// A stored tile above level 0 with its authored errors moved onto the synthesized surface (the noise offset of the
/// materials its own lattice shows; its table keeps starting at level 0).
pub(crate) fn offset_errors(
    detail: &TerrainDetail,
    rules: &TerrainMaterialRules,
    layout: &TerrainLayout,
    tile: &TerrainTile,
) -> Vec<f64> {
    let offset = detail.noise_error(0, detail.regional_amplitude(rules, layout, tile));
    let mut shifted: Vec<f64> = tile.errors().iter().map(|error| error + offset).collect();
    shifted[0] = 0.0;
    make_monotone(&mut shifted);
    shifted
}

/// Raise every entry to at least the one before it.
pub(crate) fn make_monotone(errors: &mut [f64]) {
    for index in 1..errors.len() {
        errors[index] = errors[index].max(errors[index - 1]);
    }
}

/// `smoothstep` with clamping, in `f64`.
pub(crate) fn smoothstep(edge0: f64, edge1: f64, value: f64) -> f64 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - (2.0 * t))
}

/// How much of an octave of `wavelength` level-0 samples survives subtracting its Catmull-Rom interpolation from the
/// level-0 lattice, as a fraction of its amplitude.
///
/// Midway between lattice points, Catmull-Rom weighs a sinusoid of angular frequency `ω` by
/// `(9 cos(ω / 2) - cos(3ω / 2)) / 8`; long octaves interpolate almost exactly and vanish, while octaves of two samples
/// or less interpolate to nothing and survive whole.
fn lattice_residual(wavelength: f64) -> f64 {
    let frequency = (2.0 * PI) / wavelength;
    let interpolated = (1.125 * deterministic::cos(frequency * 0.5)) - (0.125 * deterministic::cos(frequency * 1.5));
    (1.0 - interpolated).abs().min(1.0)
}

/// Detail noise at the level-0 lattice points around one synthesized tile, interpolated with Catmull-Rom.
///
/// Material weights at a lattice point come from its own sample and the central differences around it, read from the
/// level-0 tile and clamped to its apron. Tiles on either side of a level-0 boundary clamp the same shared rows, so they
/// agree at every point their samples share.
struct LatticeNoise {
    /// Level-0 tile-lattice coordinate of the first stored point.
    first: [i32; 2],
    columns: usize,
    values: Vec<f64>,
}

impl LatticeNoise {
    /// The lattice noise covering the square from `low` (level-0 tile-lattice coordinates) that spans `span` samples.
    fn new(
        detail: &TerrainDetail,
        rules: &TerrainMaterialRules,
        layout: &TerrainLayout,
        source: &TerrainTile,
        low: [f64; 2],
        span: f64,
    ) -> Self {
        // Catmull-Rom reads one point before and two after the cell holding a coordinate.
        let first = [low[0].floor() as i32 - 1, low[1].floor() as i32 - 1];
        let last = [(low[0] + span).floor() as i32 + 2, (low[1] + span).floor() as i32 + 2];
        let columns = (last[0] - first[0] + 1) as usize;
        let origin = layout.tile_origin_samples(source.key());
        let mut values = Vec::with_capacity(columns * (last[1] - first[1] + 1) as usize);
        for z in first[1]..=last[1] {
            for x in first[0]..=last[0] {
                let base = lattice_base(source, layout, x, z);
                let normal_y = 1.0 / (1.0 + (base.slope_x * base.slope_x) + (base.slope_z * base.slope_z)).sqrt();
                let weights = rules.weights(normal_y, base.height);
                values.push(detail.noise_height(origin[0] + f64::from(x), origin[1] + f64::from(z), weights));
            }
        }
        Self {
            first,
            columns,
            values,
        }
    }

    /// Catmull-Rom interpolation at level-0 tile-lattice coordinates; exact at lattice points.
    fn interpolate(&self, x: f64, z: f64) -> f64 {
        let (base_x, base_z) = (x.floor(), z.floor());
        let (weights_x, _) = catmull_rom(x - base_x);
        let (weights_z, _) = catmull_rom(z - base_z);
        let mut value = 0.0;
        for (row, weight_z) in weights_z.iter().enumerate() {
            for (column, weight_x) in weights_x.iter().enumerate() {
                let point_x = (base_x as i32 + column as i32 - 1 - self.first[0]) as usize;
                let point_z = (base_z as i32 + row as i32 - 1 - self.first[1]) as usize;
                value += self.values[(point_z * self.columns) + point_x] * weight_x * weight_z;
            }
        }
        value
    }
}

/// A level-0 sample and its central-difference slopes at an integer tile-lattice point; reads clamp to the apron.
fn lattice_base(tile: &TerrainTile, layout: &TerrainLayout, x: i32, z: i32) -> Reconstruction {
    let apron = TILE_APRON as i32;
    let last = layout.tile_cells as i32 + apron;
    let at = |px: i32, pz: i32| f64::from(tile.height(px.clamp(-apron, last), pz.clamp(-apron, last)));
    Reconstruction {
        height: at(x, z),
        slope_x: (at(x + 1, z) - at(x - 1, z)) / (2.0 * layout.spacing[0]),
        slope_z: (at(x, z + 1) - at(x, z - 1)) / (2.0 * layout.spacing[1]),
    }
}

/// A reconstructed height with its slope (height change per world unit) along X and Z.
struct Reconstruction {
    height: f64,
    slope_x: f64,
    slope_z: f64,
}

/// Catmull-Rom reconstruction of a level-0 tile at tile-lattice coordinates; reads clamp to the apron.
fn reconstruct(tile: &TerrainTile, layout: &TerrainLayout, x: f64, z: f64) -> Reconstruction {
    let cells = f64::from(layout.tile_cells);
    let base_x = x.floor().clamp(0.0, cells - 1.0);
    let base_z = z.floor().clamp(0.0, cells - 1.0);
    let (fraction_x, fraction_z) = (x - base_x, z - base_z);
    let (weights_x, derivatives_x) = catmull_rom(fraction_x);
    let (weights_z, derivatives_z) = catmull_rom(fraction_z);

    let apron = TILE_APRON as i32;
    let last = layout.tile_cells as i32 + apron;
    let mut height = 0.0;
    let mut slope_x = 0.0;
    let mut slope_z = 0.0;
    for (row, (weight_z, derivative_z)) in weights_z.iter().zip(derivatives_z).enumerate() {
        for (column, (weight_x, derivative_x)) in weights_x.iter().zip(derivatives_x).enumerate() {
            let sample_x = (base_x as i32 + column as i32 - 1).clamp(-apron, last);
            let sample_z = (base_z as i32 + row as i32 - 1).clamp(-apron, last);
            let sample = f64::from(tile.height(sample_x, sample_z));
            height += sample * weight_x * weight_z;
            slope_x += sample * derivative_x * weight_z;
            slope_z += sample * weight_x * derivative_z;
        }
    }
    Reconstruction {
        height,
        slope_x: slope_x / layout.spacing[0],
        slope_z: slope_z / layout.spacing[1],
    }
}

/// Catmull-Rom weights and their derivatives for the four samples around a fraction in `[0, 1]`.
fn catmull_rom(t: f64) -> ([f64; 4], [f64; 4]) {
    let (t2, t3) = (t * t, t * t * t);
    let weights = [
        0.5 * ((-t3) + (2.0 * t2) - t),
        0.5 * ((3.0 * t3) - (5.0 * t2) + 2.0),
        0.5 * ((-3.0 * t3) + (4.0 * t2) + t),
        0.5 * (t3 - t2),
    ];
    let derivatives = [
        0.5 * ((-3.0 * t2) + (4.0 * t) - 1.0),
        0.5 * ((9.0 * t2) - (10.0 * t)),
        0.5 * ((-9.0 * t2) + (8.0 * t) + 1.0),
        0.5 * ((3.0 * t2) - (2.0 * t)),
    ];
    (weights, derivatives)
}

/// Two-dimensional gradient noise in about `[-1, 1]`, from integer lattice hashes with quintic interpolation.
fn gradient_noise(x: f64, z: f64, seed: u32) -> f64 {
    let (cell_x, cell_z) = (x.floor(), z.floor());
    let (fraction_x, fraction_z) = (x - cell_x, z - cell_z);
    let (column, row) = (cell_x as i64, cell_z as i64);
    let corner = |dx: i64, dz: i64| {
        let (gradient_x, gradient_z) = lattice_gradient(column + dx, row + dz, seed);
        (gradient_x * (fraction_x - dx as f64)) + (gradient_z * (fraction_z - dz as f64))
    };
    let blend_x = quintic(fraction_x);
    let blend_z = quintic(fraction_z);
    let top = corner(0, 0) + ((corner(1, 0) - corner(0, 0)) * blend_x);
    let bottom = corner(0, 1) + ((corner(1, 1) - corner(0, 1)) * blend_x);
    (top + ((bottom - top) * blend_z)) * std::f64::consts::SQRT_2
}

/// One of eight unit gradients chosen by hashing a lattice corner.
fn lattice_gradient(column: i64, row: i64, seed: u32) -> (f64, f64) {
    const DIAGONAL: f64 = std::f64::consts::FRAC_1_SQRT_2;
    const GRADIENTS: [(f64, f64); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (DIAGONAL, DIAGONAL),
        (-DIAGONAL, DIAGONAL),
        (DIAGONAL, -DIAGONAL),
        (-DIAGONAL, -DIAGONAL),
    ];
    let hash = hash_corner(column as u32, row as u32, seed);
    GRADIENTS[(hash & 7) as usize]
}

/// A well-mixed 32-bit hash of a lattice corner (PCG-style output permutation).
fn hash_corner(column: u32, row: u32, seed: u32) -> u32 {
    let mut state = column.wrapping_mul(0x8DA6_B343) ^ row.wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    state = state.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// `6t^5 - 15t^4 + 10t^3`: smooth interpolation with zero first and second derivatives at the ends.
fn quintic(t: f64) -> f64 {
    t * t * t * ((t * ((t * 6.0) - 15.0)) + 10.0)
}
