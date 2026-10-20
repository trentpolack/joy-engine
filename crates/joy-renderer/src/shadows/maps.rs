// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU resources for shadows: the cascade depth array, the point-light atlas, the receiver uniform block, and per-view
//! caster matrices. Layouts here mirror the `Shadows` block and group 1 bindings in shadow-receiver.wgsl.

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::math::Mat4;

use crate::pipelines::SHADOW_FORMAT;
use crate::shadows::{
    MAX_CASCADES,
    MAX_SHADOWED_POINT_LIGHTS,
    POINT_FACES,
    ShadowOptions,
};

/// Byte stride between caster matrices; the portable minimum uniform offset alignment.
const CASTER_STRIDE: u64 = 256;

/// The smallest atlas allocated once any point light casts.
const MIN_ATLAS_SIZE: u32 = 512;

/// One cascade (`Cascade` in shadow-receiver.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct CascadeRaw {
    pub matrix: [[f32; 4]; 4],
    pub extent: [f32; 4],
}

/// One shadowed point light (`PointShadow` in shadow-receiver.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct PointShadowRaw {
    pub position_range: [f32; 4],
    pub parameters: [f32; 4],
    pub tiles: [[f32; 4]; 3],
    pub tile: [f32; 4],
}

/// Receiver uniforms (`Shadows` in shadow-receiver.wgsl). 3,440 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ShadowUniforms {
    pub counts: [f32; 4],
    pub directional: [f32; 4],
    pub parameters: [f32; 4],
    pub cascades: [CascadeRaw; MAX_CASCADES],
    pub points: [PointShadowRaw; MAX_SHADOWED_POINT_LIGHTS],
}

impl ShadowUniforms {
    /// No directional or point shadows.
    pub fn disabled() -> Self {
        Self::zeroed()
    }
}

/// The shadow receiver group: uniforms, both depth maps with a comparison sampler, and the same maps again as plain
/// floats with a nearest sampler for blocker-search reads (the GL backend allows one sampler per texture binding, cannot
/// load depth textures, and returns zero from depth textures read without a nearest sampler or comparison). With
/// `compute`, compute shaders (volumetric fog) read the group too.
pub(crate) fn receiver_layout(device: &wgpu::Device, compute: bool) -> wgpu::BindGroupLayout {
    let visibility =
        if compute { wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE } else { wgpu::ShaderStages::FRAGMENT };
    let texture = |binding, sample_type, view_dimension| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension,
            multisampled: false,
        },
        count: None,
    };
    let float = wgpu::TextureSampleType::Float {
        filterable: false,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow receiver"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<ShadowUniforms>() as u64),
                },
                count: None,
            },
            texture(1, wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2Array),
            texture(2, wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            texture(4, float, wgpu::TextureViewDimension::D2Array),
            texture(5, float, wgpu::TextureViewDimension::D2),
            wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
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

/// The cascade depth array: one attachment view per layer and one array view for sampling.
struct CascadeArray {
    // Views reference the texture; drop them first.
    layers: Vec<wgpu::TextureView>,
    sampled: wgpu::TextureView,
    _texture: wgpu::Texture,
}

impl CascadeArray {
    fn new(device: &wgpu::Device, size: u32, layer_count: u32) -> Self {
        let label = "directional shadow cascades";
        let texture = depth_texture(device, label, size, layer_count);
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

/// The point-light atlas: one 2D depth texture every face renders into through its own viewport.
struct PointAtlas {
    // The view references the texture; drop it first.
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    size: u32,
}

impl PointAtlas {
    fn new(device: &wgpu::Device, size: u32) -> Self {
        let texture = depth_texture(device, "point shadow atlas", size, 1);
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("point shadow atlas"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                ..Default::default()
            }),
            _texture: texture,
            size,
        }
    }
}

/// Owned shadow resources. With shadows disabled the maps are 1x1 placeholders that keep the receiver group valid; the
/// atlas grows, up to its configured size, as frames ask for more point-light faces, and never shrinks.
pub(crate) struct ShadowMaps {
    // Bind groups reference the views, buffers, and samplers below; drop them first.
    receiver: wgpu::BindGroup,
    caster: wgpu::BindGroup,
    cascades: CascadeArray,
    atlas: PointAtlas,
    uniforms: wgpu::Buffer,
    casters: wgpu::Buffer,
    comparison_sampler: wgpu::Sampler,
    depth_sampler: wgpu::Sampler,
    receiver_layout: wgpu::BindGroupLayout,
    caster_layout: wgpu::BindGroupLayout,
    caster_capacity: usize,
    /// The largest atlas the options allow.
    max_atlas_size: u32,
}

impl ShadowMaps {
    /// Allocate cascades for `options`; the atlas allocates on first use.
    pub fn new(
        device: &wgpu::Device,
        receiver_layout: &wgpu::BindGroupLayout,
        caster_layout: &wgpu::BindGroupLayout,
        options: Option<ShadowOptions>,
        max_texture_dimension: u32,
    ) -> Self {
        // Placeholders keep two layers: the GL backend creates single-layer textures as plain 2D, not arrays.
        let (cascade_size, cascade_count, atlas_size) = options.map_or((1, 2, 1), |options| {
            (
                options.directional_map_size(max_texture_dimension),
                options.quality.cascade_count().max(2) as u32,
                options.point_atlas_size(max_texture_dimension),
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
        let cascades = CascadeArray::new(device, cascade_size, cascade_count);
        let atlas = PointAtlas::new(device, 1);
        let casters = caster_buffer(device, MAX_CASCADES);
        let samplers = [&comparison_sampler, &depth_sampler];
        let receiver = receiver_group(device, receiver_layout, &uniforms, &cascades, &atlas, samplers);
        let caster = caster_group(device, caster_layout, &casters);
        Self {
            receiver,
            caster,
            cascades,
            atlas,
            uniforms,
            casters,
            comparison_sampler,
            depth_sampler,
            receiver_layout: receiver_layout.clone(),
            caster_layout: caster_layout.clone(),
            caster_capacity: MAX_CASCADES,
            max_atlas_size: atlas_size,
        }
    }

    /// The receiver bind group (group 1 of every lit pass).
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
        &self.cascades.layers[cascade]
    }

    /// The point-light atlas attachment.
    pub fn atlas(&self) -> &wgpu::TextureView {
        &self.atlas.view
    }

    /// The allocated atlas size, which point-light tiles are planned against.
    pub fn allocated_atlas_size(&self) -> u32 {
        self.atlas.size
    }

    /// Grow the atlas to hold six faces of each requested size (powers of two), up to its configured size.
    pub fn reserve_atlas(&mut self, device: &wgpu::Device, face_sizes: &[u32]) {
        let area: u64 = face_sizes.iter().map(|size| POINT_FACES as u64 * u64::from(*size).pow(2)).sum();
        let side = (area as f64).sqrt().ceil() as u32;
        let wanted = side.next_power_of_two().clamp(MIN_ATLAS_SIZE, self.max_atlas_size.max(MIN_ATLAS_SIZE));
        if wanted <= self.atlas.size {
            return;
        }
        self.atlas = PointAtlas::new(device, wanted);
        self.receiver = receiver_group(
            device,
            &self.receiver_layout,
            &self.uniforms,
            &self.cascades,
            &self.atlas,
            [&self.comparison_sampler, &self.depth_sampler],
        );
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
}

fn depth_texture(device: &wgpu::Device, label: &str, size: u32, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SHADOW_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
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
    cascades: &CascadeArray,
    atlas: &PointAtlas,
    [comparison_sampler, depth_sampler]: [&wgpu::Sampler; 2],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow receiver"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&cascades.sampled),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&atlas.view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(comparison_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&cascades.sampled),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&atlas.view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(depth_sampler),
            },
        ],
    })
}
