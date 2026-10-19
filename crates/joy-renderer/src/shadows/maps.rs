// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU resources for shadows: the cascade and point-light depth arrays, the receiver uniform block, and per-view
//! caster matrices. Layouts here mirror the `Shadows` block and bindings in scene-common.wgsl.

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::math::Mat4;

use crate::frame::MAX_POINT_LIGHTS;
use crate::materials::SHADOW_FORMAT;
use crate::shadows::{
    MAX_CASCADES,
    POINT_FACES,
    SHADOW_LIGHTS_PER_BATCH,
    ShadowOptions,
};

/// Byte stride between caster matrices; the portable minimum uniform offset alignment.
const CASTER_STRIDE: u64 = 256;

/// One shadowed point light (`PointShadow` in scene-common.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct PointShadowRaw {
    pub position_range: [f32; 4],
    pub parameters: [f32; 4],
}

/// Receiver uniforms (`Shadows` in scene-common.wgsl). 1,360 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ShadowUniforms {
    pub counts: [f32; 4],
    pub cascade_splits: [f32; 4],
    pub cascade_bias: [f32; 4],
    pub cascades: [[[f32; 4]; 4]; MAX_CASCADES],
    pub directional: [f32; 4],
    pub extents: [f32; 4],
    pub points: [PointShadowRaw; MAX_POINT_LIGHTS],
}

impl ShadowUniforms {
    /// No directional or point shadows.
    pub fn disabled() -> Self {
        Self::zeroed()
    }
}

/// The shadow receiver group: uniforms, both depth arrays, and their samplers. Each array is bound twice, once as depth
/// for comparisons and once as plain floats for blocker-search reads: the GL backend allows one sampler per texture
/// binding and cannot read depth textures without comparison.
pub(crate) fn receiver_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let depth_array = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    };
    let float_array = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow receiver"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<ShadowUniforms>() as u64),
                },
                count: None,
            },
            depth_array(1),
            depth_array(2),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            float_array(5),
            float_array(6),
        ],
    })
}

/// The caster group: one light view-projection selected per shadow view by dynamic offset.
pub(crate) fn caster_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow caster"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(64),
            },
            count: None,
        }],
    })
}

/// A depth texture array with one attachment view per layer and one array view for sampling.
struct DepthArray {
    // Views reference the texture; drop them first.
    layers: Vec<wgpu::TextureView>,
    sampled: wgpu::TextureView,
    _texture: wgpu::Texture,
}

impl DepthArray {
    fn new(device: &wgpu::Device, label: &str, size: u32, layer_count: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: layer_count },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layers = (0..layer_count)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some(label),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self { layers, sampled, _texture: texture }
    }
}

/// Owned shadow resources. With shadows disabled the arrays are 1x1 placeholders that keep the receiver group valid.
pub(crate) struct ShadowMaps {
    // Bind groups reference the views, buffers, and samplers below; drop them first.
    receiver: wgpu::BindGroup,
    caster: wgpu::BindGroup,
    directional: DepthArray,
    point: DepthArray,
    uniforms: wgpu::Buffer,
    casters: wgpu::Buffer,
    depth_sampler: wgpu::Sampler,
    comparison_sampler: wgpu::Sampler,
    receiver_layout: wgpu::BindGroupLayout,
    caster_layout: wgpu::BindGroupLayout,
    caster_capacity: usize,
    point_capacity: usize,
    point_size: u32,
}

impl ShadowMaps {
    /// Allocate cascades for `options`; point maps grow on first use.
    pub fn new(
        device: &wgpu::Device,
        receiver_layout: &wgpu::BindGroupLayout,
        caster_layout: &wgpu::BindGroupLayout,
        options: Option<ShadowOptions>,
        max_texture_dimension: u32,
    ) -> Self {
        // Placeholders keep two layers: the GL backend creates single-layer textures as plain 2D, not arrays.
        let (directional_size, cascades, point_size) = options.map_or((1, 2, 1), |options| {
            (
                options.directional_map_size(max_texture_dimension),
                options.quality.cascade_count() as u32,
                options.point_map_size(max_texture_dimension),
            )
        });
        let comparison_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow comparison"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let depth_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow depth"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow uniforms"),
            size: std::mem::size_of::<ShadowUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let directional = DepthArray::new(device, "directional shadow cascades", directional_size, cascades);
        let point = DepthArray::new(device, "point shadow placeholder", 1, point_layers(0));
        let casters = caster_buffer(device, MAX_CASCADES);
        let receiver = receiver_group(
            device,
            receiver_layout,
            &uniforms,
            &directional,
            &point,
            &depth_sampler,
            &comparison_sampler,
        );
        let caster = caster_group(device, caster_layout, &casters);
        Self {
            receiver,
            caster,
            directional,
            point,
            uniforms,
            casters,
            depth_sampler,
            comparison_sampler,
            receiver_layout: receiver_layout.clone(),
            caster_layout: caster_layout.clone(),
            caster_capacity: MAX_CASCADES,
            point_capacity: 0,
            point_size,
        }
    }

    /// The receiver bind group (group 1 of every surface material).
    pub fn receiver(&self) -> &wgpu::BindGroup {
        &self.receiver
    }

    /// The caster bind group; offset it with [`caster_offset`](Self::caster_offset).
    pub fn caster(&self) -> &wgpu::BindGroup {
        &self.caster
    }

    /// Dynamic offset of a view's caster matrix.
    pub fn caster_offset(view: usize) -> u32 {
        (view as u64 * CASTER_STRIDE) as u32
    }

    /// The attachment for one cascade.
    pub fn cascade_layer(&self, cascade: usize) -> &wgpu::TextureView {
        &self.directional.layers[cascade]
    }

    /// The attachment for one point-light face.
    pub fn point_layer(&self, slot: usize, face: usize) -> &wgpu::TextureView {
        &self.point.layers[(slot * POINT_FACES) + face]
    }

    /// Grow point maps to hold `lights` shadowed lights, in whole light batches.
    pub fn reserve_point_lights(&mut self, device: &wgpu::Device, lights: usize) {
        if lights <= self.point_capacity {
            return;
        }
        let capacity = lights.div_ceil(SHADOW_LIGHTS_PER_BATCH) * SHADOW_LIGHTS_PER_BATCH;
        let capacity = capacity.min(MAX_POINT_LIGHTS);
        self.point = DepthArray::new(device, "point shadow faces", self.point_size, point_layers(capacity));
        self.point_capacity = capacity;
        self.rebuild_receiver(device);
    }

    /// Upload one matrix per shadow view, growing the caster buffer as needed.
    pub fn write_casters(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, matrices: &[Mat4]) {
        if matrices.len() > self.caster_capacity {
            self.caster_capacity = matrices.len().next_power_of_two();
            self.casters = caster_buffer(device, self.caster_capacity);
            self.caster = caster_group(device, &self.caster_layout, &self.casters);
        }
        let stride = CASTER_STRIDE as usize;
        let mut bytes = vec![0_u8; matrices.len() * stride];
        for (chunk, matrix) in bytes.chunks_exact_mut(stride).zip(matrices) {
            chunk[..64].copy_from_slice(bytemuck::bytes_of(&matrix.to_cols_array()));
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.casters, 0, &bytes);
        }
    }

    /// Upload the receiver uniforms for this frame.
    pub fn write_uniforms(&self, queue: &wgpu::Queue, uniforms: &ShadowUniforms) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    fn rebuild_receiver(&mut self, device: &wgpu::Device) {
        self.receiver = receiver_group(
            device,
            &self.receiver_layout,
            &self.uniforms,
            &self.directional,
            &self.point,
            &self.depth_sampler,
            &self.comparison_sampler,
        );
    }
}

/// Layers for `lights` point lights plus one spare: the GL backend treats arrays of six or a multiple of six layers
/// as cube maps, which cannot be viewed as 2D arrays.
fn point_layers(lights: usize) -> u32 {
    (lights.max(1) * POINT_FACES + 1) as u32
}

fn caster_buffer(device: &wgpu::Device, views: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shadow casters"),
        size: views.max(1) as u64 * CASTER_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn caster_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, buffer: &wgpu::Buffer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow casters"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: wgpu::BufferSize::new(64),
            }),
        }],
    })
}

fn receiver_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    directional: &DepthArray,
    point: &DepthArray,
    depth_sampler: &wgpu::Sampler,
    comparison_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow receiver"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&directional.sampled) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&point.sampled) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(depth_sampler) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(comparison_sampler) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&directional.sampled) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&point.sampled) },
        ],
    })
}
