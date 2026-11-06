// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The instanced particle pass shared by the scene renderer and the arcade triangle renderer.
//!
//! Each frame [`ParticlePass::prepare`] reads a [`ParticleFrame`]: it culls whole batches whose bounds lie outside the
//! view, orders the instances of every source-over batch back to front (one key per particle; additive batches add
//! light in any order and are not sorted), gives each batch one view depth for the owner's transparency order, uploads
//! the images batches sample, and writes the surviving instances into one growing instance buffer. A batch then draws
//! as one instanced call of a six-vertex quad. Scratch buffers and the instance buffer persist between frames.
//!
//! Two pipelines cover the blend modes: source-over with premultiplied blending (straight-alpha emitters are
//! premultiplied in the shader, so they share it with premultiplied emitters) and one-one additive. Both test depth
//! less-or-equal without writing it. Images upload with mip chains (sRGB, sampled trilinearly with anisotropic
//! filtering and clamped edges) on first use and release once their [`TextureAsset`] is dropped everywhere.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Weak;

use joy_core::math::{
    Mat4,
    Vec3,
};

use crate::RenderError;
use crate::culling::bounds_outside_view;
use crate::gpu::GpuContext;
use crate::particles::{
    ParticleFrame,
    ParticleInstance,
};
use crate::texture::TextureAsset;
use crate::texture_mips::{
    MipmapGenerator,
    mipmapped_sampler,
};

use super::{
    DEPTH_FORMAT,
    sampler_entry,
    texture_entry,
};

/// The shared shape library every particle entry file follows.
pub(crate) const PARTICLE_SHAPES_SOURCE: &str = include_str!("../shaders/particle-shapes.wgsl");

/// The scene renderer's particle entry points (composed after the scene contract and the shape library).
pub(crate) const PARTICLE_SCENE_SOURCE: &str = include_str!("../shaders/particle-scene.wgsl");

/// The triangle renderer's particle entry points (composed after the shape library).
pub(crate) const PARTICLE_TRIANGLES_SOURCE: &str = include_str!("../shaders/particle-triangles.wgsl");

/// Vertices of the shared quad: two triangles, generated from the vertex index.
const QUAD_VERTICES: Range<u32> = 0..6;

/// Instance attributes of [`ParticleInstance`] at locations 0 through 4 (five of WebGL 2's sixteen).
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x4,
    1 => Float32x4,
    2 => Float32x4,
    3 => Unorm16x4,
    4 => Uint32,
];

/// Initial instance capacity; grows geometrically.
const INITIAL_INSTANCE_CAPACITY: usize = 1024;

/// Sprite texture format; sampling decodes sRGB to linear.
const SPRITE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Additive blending: RGB adds to the target and the target's alpha is kept.
const ADDITIVE_BLENDING: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// The camera a frame's particles are culled and ordered for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParticleView {
    /// World-to-clip matrix culling tests against.
    pub view_projection: Mat4,
    /// Eye position; ordering uses distance along `forward` from it.
    pub eye: Vec3,
    /// Unit view direction.
    pub forward: Vec3,
}

impl ParticleView {
    /// Distance of `point` in front of the eye.
    fn depth(&self, point: Vec3) -> f32 {
        (point - self.eye).dot(self.forward)
    }
}

/// One batch that survived culling, laid out in the instance buffer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreparedBatch {
    /// Instances in the pass's instance buffer, back to front for source-over batches.
    pub instances: Range<u32>,
    /// View depth of the batch's bounds center, its place in the owner's transparency order.
    pub depth: f32,
    /// Whether the batch draws with additive blending.
    pub additive: bool,
    /// The sampled image's [`TextureAsset::id`], for textured batches.
    pub texture: Option<u64>,
}

/// Counts from one [`ParticlePass::prepare`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PreparedCounts {
    /// Instances that will draw.
    pub instances: usize,
    /// Batches culled against the view.
    pub culled_batches: usize,
}

/// GPU copy of one image, released when the asset is dropped everywhere.
struct GpuTexture {
    // The bind group references the view, which references the texture; drop in that order.
    bind_group: wgpu::BindGroup,
    _view: wgpu::TextureView,
    _texture: wgpu::Texture,
    asset: Weak<dyn std::any::Any + Send + Sync>,
}

/// Owned particle pipelines, images, and instance buffer. Borrows the device per call and the open pass per draw.
pub(crate) struct ParticlePass {
    // Pipelines, the sprite group they read, and its sampler.
    over_pipeline: wgpu::RenderPipeline,
    additive_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    texture_group: u32,
    sampler: wgpu::Sampler,

    // Uploaded images keyed by asset identity, and the white texel untextured batches bind (with its asset, which keeps
    // the upload alive).
    textures: HashMap<u64, GpuTexture>,
    white: TextureAsset,

    // The instance buffer and this frame's layout of it.
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    prepared: Vec<PreparedBatch>,

    // Scratch kept between frames: instances in upload order and one sort key per instance.
    upload: Vec<ParticleInstance>,
    order: Vec<(f32, u32)>,
}

impl ParticlePass {
    /// Compile both pipelines from `source` (the shape library plus an entry file) for `scene_format`. The pipeline
    /// layout is `leading_layouts` followed by the sprite texture group.
    pub fn new(
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        leading_layouts: &[&wgpu::BindGroupLayout],
        source: String,
        scene_format: wgpu::TextureFormat,
    ) -> Self {
        let device = gpu.device();
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle sprite"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let mut layouts: Vec<Option<&wgpu::BindGroupLayout>> = leading_layouts.iter().copied().map(Some).collect();
        layouts.push(Some(&texture_layout));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &layouts,
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = |entry_point: &str, blend: wgpu::BlendState| {
            particle_pipeline(device, &layout, &module, entry_point, blend, scene_format)
        };
        let over_pipeline = pipeline("fragmentOver", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let additive_pipeline = pipeline("fragmentAdditive", ADDITIVE_BLENDING);

        let white = TextureAsset::from_rgba8(1, 1, vec![255; 4]).expect("one white texel is a valid image");
        let mut pass = Self {
            over_pipeline,
            additive_pipeline,
            texture_layout,
            texture_group: leading_layouts.len() as u32,
            sampler: mipmapped_sampler(device, "particle sprite", wgpu::AddressMode::ClampToEdge),
            textures: HashMap::new(),
            white: white.clone(),
            instance_buffer: instance_buffer(device, INITIAL_INSTANCE_CAPACITY),
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            prepared: Vec::new(),
            upload: Vec::new(),
            order: Vec::new(),
        };
        pass.upload_texture(gpu, mipmaps, &white);
        pass
    }

    /// Cull, order, and upload `frame` for `view`, replacing the previous frame's batches; `None` draws nothing.
    ///
    /// Fails before uploading anything when an image exceeds the device's largest 2D texture.
    pub fn prepare(
        &mut self,
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        frame: Option<&ParticleFrame>,
        view: &ParticleView,
    ) -> Result<PreparedCounts, RenderError> {
        self.prepared.clear();
        self.upload.clear();
        let Some(frame) = frame else {
            return Ok(PreparedCounts::default());
        };
        let limit = gpu.capabilities().max_texture_dimension;
        if let Some(texture) =
            frame.textures().iter().find(|texture| texture.width() > limit || texture.height() > limit)
        {
            return Err(RenderError::InvalidFrame(format!(
                "a {}x{} texture exceeds this device's {limit}-pixel limit",
                texture.width(),
                texture.height()
            )));
        }

        let mut counts = PreparedCounts::default();
        for batch in frame.batches() {
            if bounds_outside_view(batch.bounds(), view.view_projection) {
                counts.culled_batches += 1;
                continue;
            }
            let range = batch.instances();
            let instances = &frame.instances()[range.start as usize..range.end as usize];
            let start = self.upload.len() as u32;
            if batch.is_additive() {
                self.upload.extend_from_slice(instances);
            } else {
                // Farthest first; equal depths keep generation order, so the order is deterministic.
                self.order.clear();
                self.order.extend(
                    instances
                        .iter()
                        .enumerate()
                        .map(|(index, instance)| (view.depth(instance.position()), index as u32)),
                );
                self.order.sort_unstable_by(|left, right| right.0.total_cmp(&left.0).then(left.1.cmp(&right.1)));
                self.upload.extend(self.order.iter().map(|(_, index)| instances[*index as usize]));
            }

            let texture = batch.texture().map(|slot| {
                let image = &frame.textures()[slot as usize];
                self.upload_texture(gpu, mipmaps, image);
                image.id()
            });
            self.prepared.push(PreparedBatch {
                instances: start..(self.upload.len() as u32),
                depth: view.depth(batch.bounds().center()),
                additive: batch.is_additive(),
                texture,
            });
        }
        counts.instances = self.upload.len();
        self.write_instances(gpu);
        Ok(counts)
    }

    /// This frame's batches after [`prepare`](Self::prepare), in frame order.
    pub fn batches(&self) -> &[PreparedBatch] {
        &self.prepared
    }

    /// Record prepared batch `index` into the open pass, which must already bind the groups before the sprite group.
    /// Returns the instances drawn (zero for a missing batch or image).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) -> u32 {
        let Some(batch) = self.prepared.get(index) else {
            return 0;
        };
        let group = match batch.texture {
            Some(id) => match self.textures.get(&id) {
                Some(texture) => &texture.bind_group,
                None => return 0,
            },
            None => match self.textures.get(&self.white.id()) {
                Some(texture) => &texture.bind_group,
                None => return 0,
            },
        };
        pass.set_pipeline(if batch.additive { &self.additive_pipeline } else { &self.over_pipeline });
        pass.set_bind_group(self.texture_group, group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(QUAD_VERTICES, batch.instances.clone());
        batch.instances.len() as u32
    }

    /// Release uploads whose images no longer exist anywhere.
    pub fn release_dropped(&mut self) {
        self.textures.retain(|_, texture| texture.asset.strong_count() > 0);
    }

    /// Upload an image with its mip chain the first time it is drawn.
    fn upload_texture(&mut self, gpu: &GpuContext, mipmaps: &mut MipmapGenerator, asset: &TextureAsset) {
        if self.textures.contains_key(&asset.id()) {
            return;
        }
        let device = gpu.device();
        let texture = mipmaps.upload(gpu, "particle texture", asset, SPRITE_FORMAT);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle texture"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.textures.insert(
            asset.id(),
            GpuTexture {
                bind_group,
                _view: view,
                _texture: texture,
                asset: asset.downgrade(),
            },
        );
    }

    /// Upload this frame's instances in drawing order, growing the buffer geometrically.
    fn write_instances(&mut self, gpu: &GpuContext) {
        if self.upload.is_empty() {
            return;
        }
        if self.upload.len() > self.instance_capacity {
            self.instance_capacity = self.upload.len().next_power_of_two();
            self.instance_buffer = instance_buffer(gpu.device(), self.instance_capacity);
        }
        gpu.queue().write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.upload));
    }
}

/// One particle pipeline: the shared quad expanded per instance, depth tested without writes, and `blend`.
fn particle_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fragment_entry: &str,
    blend: wgpu::BlendState,
    scene_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ParticleInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES,
            })],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: scene_format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle instances"),
        size: (capacity * std::mem::size_of::<ParticleInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
