// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The light textures the frame group binds (`pointLights`, `lightClusters`, and `clusterLightIndices` in
//! scene-common.wgsl). They are allocated once at their largest size, so bind groups built over them never go stale;
//! each frame uploads only the rows it uses.

use crate::frame::MAX_POINT_LIGHTS;
use crate::lighting::clusters::{
    CLUSTER_COLUMNS,
    CLUSTER_INDEX_ROW,
    CLUSTER_ROWS,
    CLUSTER_SLICES,
    LightClusters,
    MAX_CLUSTER_INDICES,
};

/// Owned light textures and their views.
pub(crate) struct LightTextures {
    // Views reference the textures; drop them first.
    lights_view: wgpu::TextureView,
    clusters_view: wgpu::TextureView,
    indices_view: wgpu::TextureView,
    lights: wgpu::Texture,
    clusters: wgpu::Texture,
    indices: wgpu::Texture,
}

impl LightTextures {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let lights = texture(device, "point lights", MAX_POINT_LIGHTS as u32, 2, wgpu::TextureFormat::Rgba32Float);
        let clusters = texture(
            device,
            "light clusters",
            CLUSTER_COLUMNS * CLUSTER_ROWS,
            CLUSTER_SLICES,
            wgpu::TextureFormat::Rg32Uint,
        );
        let indices = texture(
            device,
            "cluster light indices",
            CLUSTER_INDEX_ROW,
            MAX_CLUSTER_INDICES as u32 / CLUSTER_INDEX_ROW,
            wgpu::TextureFormat::R16Uint,
        );
        Self {
            lights_view: lights.create_view(&wgpu::TextureViewDescriptor::default()),
            clusters_view: clusters.create_view(&wgpu::TextureViewDescriptor::default()),
            indices_view: indices.create_view(&wgpu::TextureViewDescriptor::default()),
            lights,
            clusters,
            indices,
        }
    }

    /// Bind-group entries for the frame group's light bindings, starting at `first_binding`.
    pub(crate) fn entries(&self, first_binding: u32) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: first_binding,
                resource: wgpu::BindingResource::TextureView(&self.lights_view),
            },
            wgpu::BindGroupEntry {
                binding: first_binding + 1,
                resource: wgpu::BindingResource::TextureView(&self.clusters_view),
            },
            wgpu::BindGroupEntry {
                binding: first_binding + 2,
                resource: wgpu::BindingResource::TextureView(&self.indices_view),
            },
        ]
    }

    /// Layout entries matching [`entries`](Self::entries), visible to `visibility`.
    pub(crate) fn layout_entries(
        first_binding: u32,
        visibility: wgpu::ShaderStages,
    ) -> [wgpu::BindGroupLayoutEntry; 3] {
        let entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        [
            entry(
                first_binding,
                wgpu::TextureSampleType::Float {
                    filterable: false,
                },
            ),
            entry(first_binding + 1, wgpu::TextureSampleType::Uint),
            entry(first_binding + 2, wgpu::TextureSampleType::Uint),
        ]
    }

    /// Upload one frame's lights and cluster lists.
    pub(crate) fn upload(&self, queue: &wgpu::Queue, clusters: &LightClusters) {
        let light_count = clusters.positions.len().min(MAX_POINT_LIGHTS);
        if light_count > 0 {
            let mut rows = Vec::with_capacity(light_count * 2);
            rows.extend_from_slice(&clusters.positions[..light_count]);
            rows.extend_from_slice(&clusters.colors[..light_count]);
            write(queue, &self.lights, bytemuck::cast_slice(&rows), light_count as u32 * 16, (light_count as u32, 2));
        }

        let width = CLUSTER_COLUMNS * CLUSTER_ROWS;
        write(queue, &self.clusters, bytemuck::cast_slice(&clusters.ranges), width * 8, (width, CLUSTER_SLICES));

        // Whole rows of indices, the last one padded.
        let used = clusters.indices.len().min(MAX_CLUSTER_INDICES);
        if used > 0 {
            let rows = used.div_ceil(CLUSTER_INDEX_ROW as usize);
            let mut padded = clusters.indices[..used].to_vec();
            padded.resize(rows * CLUSTER_INDEX_ROW as usize, 0);
            write(
                queue,
                &self.indices,
                bytemuck::cast_slice(&padded),
                CLUSTER_INDEX_ROW * 2,
                (CLUSTER_INDEX_ROW, rows as u32),
            );
        }
    }
}

fn texture(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Write `bytes` into the top-left `size` texels of `texture`.
fn write(queue: &wgpu::Queue, texture: &wgpu::Texture, bytes: &[u8], bytes_per_row: u32, size: (u32, u32)) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(size.1),
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
}
