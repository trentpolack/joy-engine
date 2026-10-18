// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Clustered point lights for volumetric lighting: lights move
//! into view space, then bin into screen tiles and exponential depth slices, uploaded as storage buffers.

use glam::Mat4;

use crate::PointLight;

/// Lights the volume pass reads.
const MAX_POINT_LIGHTS: usize = 32;
/// Lights kept per cluster.
pub(crate) const MAX_LIGHTS_PER_CLUSTER: u32 = 8;
const MAX_CLUSTER_COUNT_X: u32 = 16;
const MAX_CLUSTER_COUNT_Y: u32 = 9;
const CLUSTER_COUNT_Z: u32 = 16;
/// Screen tile size in physical pixels.
const TILE_SIZE: u32 = 128;
const LIGHT_FLOATS: usize = 8;

/// Camera values the binning needs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VolumeCamera {
    pub(crate) view: Mat4,
    pub(crate) projection: Mat4,
    pub(crate) near: f32,
    pub(crate) far: f32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Buffers and cluster dimensions for the volumetric trace.
pub(crate) struct ClusterBindings<'a> {
    pub(crate) point_lights: &'a wgpu::Buffer,
    pub(crate) cluster_light_counts: &'a wgpu::Buffer,
    pub(crate) cluster_light_indices: &'a wgpu::Buffer,
    pub(crate) cluster_count: [u32; 3],
    pub(crate) point_light_count: u32,
}

/// Binning results for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneVolumeStats {
    /// Lights supplied.
    pub source_light_count: usize,
    /// Lights uploaded.
    pub point_light_count: usize,
    /// Lights beyond the upload limit.
    pub dropped_light_count: usize,
    /// Light-cluster pairs beyond the per-cluster limit.
    pub dropped_cluster_light_count: usize,
    /// Clusters this frame.
    pub cluster_count: usize,
}

/// Storage buffers reused across frames.
pub(crate) struct VolumeLights {
    light_data: Vec<f32>,
    counts: Vec<u32>,
    indices: Vec<u32>,
    point_lights: wgpu::Buffer,
    cluster_light_counts: wgpu::Buffer,
    cluster_light_indices: wgpu::Buffer,
    capacity: usize,
    stats: SceneVolumeStats,
}

impl VolumeLights {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            light_data: vec![0.0; MAX_POINT_LIGHTS * LIGHT_FLOATS],
            counts: vec![0; 1],
            indices: vec![0; MAX_LIGHTS_PER_CLUSTER as usize],
            point_lights: storage_buffer(device, "scene volume point lights", MAX_POINT_LIGHTS * LIGHT_FLOATS * 4),
            cluster_light_counts: storage_buffer(device, "scene volume cluster counts", 4),
            cluster_light_indices: storage_buffer(
                device,
                "scene volume cluster indices",
                MAX_LIGHTS_PER_CLUSTER as usize * 4,
            ),
            capacity: 1,
            stats: SceneVolumeStats::default(),
        }
    }

    /// Latest binning statistics.
    pub(crate) fn stats(&self) -> SceneVolumeStats {
        self.stats
    }

    /// Bin `lights` for `camera` and upload the buffers.
    pub(crate) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lights: &[PointLight],
        camera: &VolumeCamera,
    ) -> ClusterBindings<'_> {
        let point_light_count = lights.len().min(MAX_POINT_LIGHTS);
        self.light_data.fill(0.0);
        for (index, light) in lights.iter().take(point_light_count).enumerate() {
            let view = camera.view.transform_point3(light.position);
            let offset = index * LIGHT_FLOATS;
            self.light_data[offset..offset + LIGHT_FLOATS].copy_from_slice(&[
                view.x,
                view.y,
                view.z,
                light.range.max(f32::MIN_POSITIVE),
                light.color[0].max(0.0),
                light.color[1].max(0.0),
                light.color[2].max(0.0),
                1.0,
            ]);
        }

        let count_x = MAX_CLUSTER_COUNT_X.min(camera.width.div_ceil(TILE_SIZE)).max(1);
        let count_y = MAX_CLUSTER_COUNT_Y.min(camera.height.div_ceil(TILE_SIZE)).max(1);
        let count_z = CLUSTER_COUNT_Z;
        let cluster_count = (count_x * count_y * count_z) as usize;
        self.ensure_storage(device, cluster_count);
        self.counts[..cluster_count].fill(0);
        let per_cluster = MAX_LIGHTS_PER_CLUSTER as usize;
        self.indices[..cluster_count * per_cluster].fill(0);

        let horizontal = tile_planes(camera.projection, count_x, 0);
        let vertical = tile_planes(camera.projection, count_y, 1);
        let depth_ratio = f64::from(camera.far) / f64::from(camera.near);
        let mut dropped = 0;
        for light_index in 0..point_light_count {
            let offset = light_index * LIGHT_FLOATS;
            let [x, y, z, radius] = [0, 1, 2, 3].map(|component| f64::from(self.light_data[offset + component]));
            for tile_z in 0..count_z {
                let near = f64::from(camera.near) * depth_ratio.powf(f64::from(tile_z) / f64::from(count_z));
                let far = f64::from(camera.near) * depth_ratio.powf(f64::from(tile_z + 1) / f64::from(count_z));
                if -z + radius < near || -z - radius > far {
                    continue;
                }
                for tile_y in 0..count_y {
                    if !intersects_tile(&vertical, tile_y as usize, x, y, z, radius) {
                        continue;
                    }
                    for tile_x in 0..count_x {
                        if !intersects_tile(&horizontal, tile_x as usize, x, y, z, radius) {
                            continue;
                        }
                        let cluster = (tile_x + count_x * (tile_y + count_y * tile_z)) as usize;
                        let candidates = self.counts[cluster] as usize;
                        if candidates >= per_cluster {
                            dropped += 1;
                            continue;
                        }
                        self.indices[cluster * per_cluster + candidates] = light_index as u32;
                        self.counts[cluster] = candidates as u32 + 1;
                    }
                }
            }
        }

        queue.write_buffer(&self.point_lights, 0, bytemuck::cast_slice(&self.light_data));
        queue.write_buffer(&self.cluster_light_counts, 0, bytemuck::cast_slice(&self.counts[..cluster_count]));
        let indices = &self.indices[..cluster_count * per_cluster];
        queue.write_buffer(&self.cluster_light_indices, 0, bytemuck::cast_slice(indices));
        self.stats = SceneVolumeStats {
            source_light_count: lights.len(),
            point_light_count,
            dropped_light_count: lights.len() - point_light_count,
            dropped_cluster_light_count: dropped,
            cluster_count,
        };
        ClusterBindings {
            point_lights: &self.point_lights,
            cluster_light_counts: &self.cluster_light_counts,
            cluster_light_indices: &self.cluster_light_indices,
            cluster_count: [count_x, count_y, count_z],
            point_light_count: point_light_count as u32,
        }
    }

    /// Grow the cluster buffers by doubling, up to the maximum cluster count.
    fn ensure_storage(&mut self, device: &wgpu::Device, cluster_count: usize) {
        if cluster_count <= self.capacity {
            return;
        }
        let maximum = (MAX_CLUSTER_COUNT_X * MAX_CLUSTER_COUNT_Y * CLUSTER_COUNT_Z) as usize;
        let mut capacity = self.capacity.max(1);
        while capacity < cluster_count {
            capacity *= 2;
        }
        let capacity = capacity.min(maximum);
        let per_cluster = MAX_LIGHTS_PER_CLUSTER as usize;
        self.counts = vec![0; capacity];
        self.indices = vec![0; capacity * per_cluster];
        self.cluster_light_counts = storage_buffer(device, "scene volume cluster counts", capacity * 4);
        self.cluster_light_indices = storage_buffer(device, "scene volume cluster indices", capacity * per_cluster * 4);
        self.capacity = capacity;
    }
}

fn storage_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Normalized side planes of each screen tile along one axis (0 horizontal, 1 vertical), in view space.
fn tile_planes(projection: Mat4, count: u32, axis: usize) -> Vec<[f64; 8]> {
    let columns = projection.to_cols_array();
    (0..count)
        .map(|tile| {
            let tile = f64::from(tile);
            let count = f64::from(count);
            let (minimum, maximum) = if axis == 0 {
                (2.0 * tile / count - 1.0, 2.0 * (tile + 1.0) / count - 1.0)
            } else {
                (1.0 - 2.0 * (tile + 1.0) / count, 1.0 - 2.0 * tile / count)
            };
            let mut planes = [0.0; 8];
            for side in 0..2 {
                let sign = if side == 0 { 1.0 } else { -1.0 };
                let boundary = if side == 0 { minimum } else { maximum };
                for component in 0..4 {
                    planes[side * 4 + component] = sign
                        * (f64::from(columns[component * 4 + axis]) - boundary * f64::from(columns[component * 4 + 3]));
                }
                let length =
                    planes[side * 4].hypot(planes[side * 4 + 1]).hypot(planes[side * 4 + 2]).max(f64::MIN_POSITIVE);
                for component in 0..4 {
                    planes[side * 4 + component] /= length;
                }
            }
            planes
        })
        .collect()
}

/// Whether a view-space sphere touches a tile.
fn intersects_tile(planes: &[[f64; 8]], tile: usize, x: f64, y: f64, z: f64, radius: f64) -> bool {
    (0..2).all(|side| {
        let plane = &planes[tile][side * 4..side * 4 + 4];
        plane[0] * x + plane[1] * y + plane[2] * z + plane[3] >= -radius
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perspective;

    #[test]
    fn lights_bin_into_the_tiles_and_slices_they_touch() {
        let projection = perspective(60_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
        let horizontal = tile_planes(projection, 4, 0);
        // A light straight ahead touches the middle tiles but not the outermost ones.
        let (x, y, z, radius) = (0.0, 0.0, -10.0, 0.5);
        assert!(intersects_tile(&horizontal, 1, x, y, z, radius) && intersects_tile(&horizontal, 2, x, y, z, radius));
        assert!(!intersects_tile(&horizontal, 0, x, y, z, radius) && !intersects_tile(&horizontal, 3, x, y, z, radius));
        // A light toward the right edge (NDC x about 0.78) reaches only the right tile; one past the edge reaches none.
        assert!(intersects_tile(&horizontal, 3, 8.0, 0.0, -10.0, 0.5));
        assert!(!intersects_tile(&horizontal, 2, 8.0, 0.0, -10.0, 0.5));
        assert!((0..4).all(|tile| !intersects_tile(&horizontal, tile, 14.0, 0.0, -10.0, 0.5)));
    }
}
