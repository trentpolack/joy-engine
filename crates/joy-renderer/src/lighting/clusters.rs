// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Light clusters: the frame's point lights binned into a grid of view-aligned cells, so each surface reads only the
//! lights whose range reaches its cell.
//!
//! The grid divides the view into [`CLUSTER_COLUMNS`] by [`CLUSTER_ROWS`] screen tiles and [`CLUSTER_SLICES`] depth
//! slices. Slices are spaced exponentially from the camera's near plane to the farthest depth any light reaches, so
//! cells stay roughly cube-shaped at every distance. A light joins every cell its range sphere can touch: for each slice
//! it overlaps, the sphere's cross-section within that slice is bounded by a box whose projected rectangle selects the
//! tiles. The test is conservative (a light may be listed where it contributes nothing, never the reverse), and it
//! uses the stable camera, matching the shader lookup (`lightCluster` in scene-common.wgsl).
//!
//! Lists keep the frame's light order, strongest first. When the combined lists outgrow [`MAX_CLUSTER_INDICES`], the
//! weakest lights lose their cells first, and the frame reports how many references it dropped.

use joy_core::math::{
    Vec3,
    Vec4,
};
use joy_render_types::PointLight;

use crate::camera::CameraFrame;

/// Screen tiles across the view.
pub(crate) const CLUSTER_COLUMNS: u32 = 16;
/// Screen tiles down the view.
pub(crate) const CLUSTER_ROWS: u32 = 9;
/// Depth slices from the near plane to the farthest light reach.
pub(crate) const CLUSTER_SLICES: u32 = 24;
/// Cells in the grid.
pub(crate) const CLUSTER_COUNT: usize = (CLUSTER_COLUMNS * CLUSTER_ROWS * CLUSTER_SLICES) as usize;
/// Entries per row of the index texture; must match `CLUSTER_INDEX_ROW` in scene-common.wgsl.
pub(crate) const CLUSTER_INDEX_ROW: u32 = 1024;
/// Light references every cell can hold together; 128 rows of the index texture.
pub(crate) const MAX_CLUSTER_INDICES: usize = (CLUSTER_INDEX_ROW * 128) as usize;

/// Nearest depth a slice boundary may sit at, so cameras with tiny or zero near planes keep usable slices.
const MIN_CLUSTER_NEAR: f32 = 0.01;
/// Relative padding on every boundary test, so rounding in the shader never places a position past a listed cell.
const BOUNDARY_PADDING: f32 = 0.0001;

/// How the shader turns a view depth into a slice: `floor(log2(depth) * scale + bias)`, for depths up to `far`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ClusterDepth {
    pub scale: f32,
    pub bias: f32,
    pub far: f32,
}

impl ClusterDepth {
    /// Slices from `near` to `far`, both positive view depths.
    fn new(near: f32, far: f32) -> Self {
        let scale = CLUSTER_SLICES as f32 / (far / near).log2();
        Self {
            scale,
            bias: -near.log2() * scale,
            far,
        }
    }

    /// The slice holding a view depth, clamped to the grid.
    fn slice(self, depth: f32) -> u32 {
        let slice = (depth.max(f32::MIN_POSITIVE).log2() * self.scale) + self.bias;
        (slice.max(0.0) as u32).min(CLUSTER_SLICES - 1)
    }

    /// The view depth where `slice` begins.
    fn boundary(self, slice: u32) -> f32 {
        ((slice as f32 - self.bias) / self.scale).exp2()
    }
}

/// The frame's lights and their cluster lists, laid out for the light textures.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LightClusters {
    /// Per light: world position and range (row 0 of the light texture).
    pub positions: Vec<[f32; 4]>,
    /// Per light: linear color and shadow slot plus one, or zero without a shadow (row 1).
    pub colors: Vec<[f32; 4]>,
    /// Per cell, indexed `column + row * CLUSTER_COLUMNS + slice * CLUSTER_COLUMNS * CLUSTER_ROWS`: the first entry of
    /// its list in `indices` and the list's length.
    pub ranges: Vec<[u32; 2]>,
    /// Every cell's list of light indices, concatenated in cell order.
    pub indices: Vec<u16>,
    /// The depth slicing the lists were built with.
    pub depth: ClusterDepth,
    /// Light references left out because the lists were full.
    pub dropped: usize,
}

impl LightClusters {
    /// Bin `lights` (strongest first) for `camera`. `shadow_slots` holds each light's shadow slot, when it has one.
    pub(crate) fn build(lights: &[PointLight], shadow_slots: &[Option<usize>], camera: &CameraFrame) -> Self {
        let mut clusters = Self {
            positions: Vec::with_capacity(lights.len()),
            colors: Vec::with_capacity(lights.len()),
            ranges: vec![[0, 0]; CLUSTER_COUNT],
            ..Self::default()
        };
        for (index, light) in lights.iter().enumerate() {
            let slot = shadow_slots.get(index).copied().flatten().map_or(0.0, |slot| (slot + 1) as f32);
            clusters.positions.push(light.position.extend(light.range).to_array());
            clusters.colors.push(light.color.with_alpha(slot).to_array());
        }

        // Slices end at the farthest depth any light reaches, within the camera's far plane.
        let near = camera.near.max(MIN_CLUSTER_NEAR);
        let reach = lights
            .iter()
            .map(|light| view_depth(camera, light.position) + light.range)
            .fold(near * 2.0, f32::max)
            .min(camera.far.max(near * 2.0));
        clusters.depth = ClusterDepth::new(near, reach);

        // Light references in light order, then counted into each cell's list (a stable counting sort keeps every
        // list strongest first).
        let mut references: Vec<(u32, u16)> = Vec::new();
        for (index, light) in lights.iter().enumerate() {
            let before = references.len();
            bin_light(camera, clusters.depth, light, |cell| references.push((cell, index as u16)));
            if references.len() > MAX_CLUSTER_INDICES {
                clusters.dropped += references.len() - before;
                references.truncate(before);
            }
        }
        for (cell, _) in &references {
            clusters.ranges[*cell as usize][1] += 1;
        }
        let mut next = 0;
        for range in &mut clusters.ranges {
            range[0] = next;
            next += range[1];
        }
        let mut cursor: Vec<u32> = clusters.ranges.iter().map(|range| range[0]).collect();
        clusters.indices = vec![0; references.len()];
        for (cell, light) in references {
            let position = &mut cursor[cell as usize];
            clusters.indices[*position as usize] = light;
            *position += 1;
        }
        clusters
    }

    /// Light references across every cell.
    pub(crate) fn reference_count(&self) -> usize {
        self.indices.len()
    }
}

/// Visit every cell `light`'s range sphere can touch.
fn bin_light(camera: &CameraFrame, depth: ClusterDepth, light: &PointLight, mut visit: impl FnMut(u32)) {
    let center = camera.view.transform_point3(light.position);
    let center_depth = -center.z;
    let radius = light.range;
    let nearest = (center_depth - radius).max(depth.boundary(0));
    let farthest = (center_depth + radius).min(depth.far);
    if nearest > farthest {
        return;
    }

    let first = depth.slice(nearest * (1.0 - BOUNDARY_PADDING));
    let last = depth.slice(farthest * (1.0 + BOUNDARY_PADDING));
    for slice in first..=last {
        // The part of the sphere inside this slice: its depth span and the widest cross-section within it.
        let slab_near = nearest.max(depth.boundary(slice));
        let slab_far = farthest.min(depth.boundary(slice + 1));
        if slab_near > slab_far {
            continue;
        }
        let gap = if center_depth < slab_near {
            slab_near - center_depth
        } else if center_depth > slab_far {
            center_depth - slab_far
        } else {
            0.0
        };
        let cross_section = (radius * radius - gap * gap).max(0.0).sqrt();
        let Some((columns, rows)) = projected_tiles(camera, center, cross_section, slab_near, slab_far) else {
            continue;
        };
        let slice_base = slice * CLUSTER_COLUMNS * CLUSTER_ROWS;
        for row in rows.0..=rows.1 {
            for column in columns.0..=columns.1 {
                visit(slice_base + (row * CLUSTER_COLUMNS) + column);
            }
        }
    }
}

/// The inclusive column and row ranges a view-space box covers on screen: `center.xy` plus or minus `half_width`,
/// between view depths `near` and `far`. `None` when the box lies outside the view.
fn projected_tiles(
    camera: &CameraFrame,
    center: Vec3,
    half_width: f32,
    near: f32,
    far: f32,
) -> Option<((u32, u32), (u32, u32))> {
    let mut minimum = [f32::INFINITY; 2];
    let mut maximum = [f32::NEG_INFINITY; 2];
    for depth in [near, far] {
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let corner = Vec4::new(center.x + (x * half_width), center.y + (y * half_width), -depth, 1.0);
            let clip = camera.projection * corner;
            if clip.w <= 0.0 {
                return Some(((0, CLUSTER_COLUMNS - 1), (0, CLUSTER_ROWS - 1)));
            }
            // Screen coordinates with a top-left origin, matching the shader.
            let screen = [(clip.x / clip.w) * 0.5 + 0.5, 0.5 - ((clip.y / clip.w) * 0.5)];
            for axis in 0..2 {
                minimum[axis] = minimum[axis].min(screen[axis]);
                maximum[axis] = maximum[axis].max(screen[axis]);
            }
        }
    }
    if maximum[0] < 0.0 || maximum[1] < 0.0 || minimum[0] > 1.0 || minimum[1] > 1.0 {
        return None;
    }
    let tiles = |low: f32, high: f32, count: u32| {
        let scale = count as f32;
        let first = ((low - BOUNDARY_PADDING).clamp(0.0, 1.0) * scale) as u32;
        let last = ((high + BOUNDARY_PADDING).clamp(0.0, 1.0) * scale) as u32;
        (first.min(count - 1), last.min(count - 1))
    };
    Some((tiles(minimum[0], maximum[0], CLUSTER_COLUMNS), tiles(minimum[1], maximum[1], CLUSTER_ROWS)))
}

/// Distance of `point` in front of the camera.
fn view_depth(camera: &CameraFrame, point: Vec3) -> f32 {
    -camera.view.transform_point3(point).z
}

#[cfg(test)]
#[path = "clusters.test.rs"]
mod tests;
