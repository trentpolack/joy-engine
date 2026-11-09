// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The scene renderer's instanced particle pass.
//!
//! Each frame [`ParticlePass::prepare`] reads a [`ParticleFrame`]: it culls whole batches whose bounds lie outside the
//! view, orders the instances of every source-over batch back to front (one key per particle; additive batches add
//! light in any order and are not sorted), gives each batch the view depth of its sort point for the renderer's
//! transparency order, uploads the images batches sample, and fills one growing instance buffer with the surviving
//! instances in drawing order. A batch then draws as one instanced call of a six-vertex quad. Scratch buffers and the
//! instance buffer persist between frames.
//!
//! Ordering runs on the GPU where compute shaders do ([`ParticleSorter`]): the instances upload unsorted and a compute
//! pass recorded into the frame's encoder sorts them into the instance buffer before any render pass reads it. WebGL 2
//! and adapters without compute sort on the CPU before uploading. Both produce the same order for the same depths.
//!
//! Two pipelines cover the blend modes: source-over with premultiplied blending (straight-alpha emitters are
//! premultiplied in the shader, so they share it with premultiplied emitters) and one-one additive. Both test depth
//! less-or-equal without writing it. Batches with soft particles draw with a second pair that also reads the scene
//! depth (group 3) to fade near opaque geometry, while transparency draws in its own pass over a read-only depth
//! attachment; WebGL 2 cannot read the depth buffer it tests against, so it has only the hard pair and draws soft
//! particles hard. Lit particles read the frame's lights from group 0, which every particle pipeline binds. Images
//! upload with mip chains (sRGB, sampled trilinearly with anisotropic filtering and clamped edges) on first use and
//! release once their [`TextureAsset`] is dropped everywhere.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Weak;

use joy_core::math::{
    Mat4,
    Vec3,
};

use crate::RenderError;
use crate::culling::bounds_outside_view;
use crate::gpu::{
    GpuBackend,
    GpuContext,
};
use crate::particles::{
    ParticleFrame,
    ParticleInstance,
};
use crate::texture::TextureAsset;
use crate::texture_mips::{
    MipmapGenerator,
    mipmapped_sampler,
};

use super::particle_sort::{
    ParticleSorter,
    UNSORTED_SEGMENT,
};
use super::{
    DEPTH_FORMAT,
    sampler_entry,
    texture_entry,
};

/// The shape library every particle module composes after the scene contract.
pub(crate) const PARTICLE_SHAPES_SOURCE: &str = include_str!("../shaders/particle-shapes.wgsl");

/// The soft fade for pipelines that read the scene depth (group 3).
pub(crate) const PARTICLE_DEPTH_SOURCE: &str = include_str!("../shaders/particle-depth.wgsl");

/// The hard stand-in for the soft fade, without a depth binding.
pub(crate) const PARTICLE_DEPTH_NONE_SOURCE: &str = include_str!("../shaders/particle-depth-none.wgsl");

/// The scene renderer's particle entry points.
pub(crate) const PARTICLE_SCENE_SOURCE: &str = include_str!("../shaders/particle-scene.wgsl");

/// Vertices of the shared quad: two triangles, generated from the vertex index.
const QUAD_VERTICES: Range<u32> = 0..6;

/// Instance attributes of [`ParticleInstance`] at locations 0 through 5 (six of WebGL 2's sixteen).
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x4,
    1 => Float32x4,
    2 => Float32x4,
    3 => Unorm16x4,
    4 => Uint32,
    5 => Float32,
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
    /// View depth of the batch's sort point, its place in the owner's transparency order.
    pub depth: f32,
    /// View depth of the batch's bounds center, which orders batches that share a sort point among themselves.
    pub bounds_depth: f32,
    /// Whether the batch draws with additive blending.
    pub additive: bool,
    /// The sampled image's [`TextureAsset::id`], for textured batches.
    pub texture: Option<u64>,
    /// Whether the batch fades against the scene depth.
    pub soft: bool,
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

/// The source-over and additive pipelines of one particle module.
struct BlendPipelines {
    over: wgpu::RenderPipeline,
    additive: wgpu::RenderPipeline,
}

/// The soft pipelines and the scene-depth layout they read, on devices that can sample depth during transparency.
struct SoftPipelines {
    pipelines: BlendPipelines,
    depth_layout: wgpu::BindGroupLayout,
}

/// Owned particle pipelines, images, sorter, and instance buffer. Borrows the device per call, the frame's encoder
/// while preparing, and the open pass per draw.
pub(crate) struct ParticlePass {
    // Pipelines: hard, and soft where depth can be read; the sprite group they read, and its sampler.
    hard: BlendPipelines,
    soft: Option<SoftPipelines>,
    texture_layout: wgpu::BindGroupLayout,
    texture_group: u32,
    sampler: wgpu::Sampler,
    // This frame's scene-depth group for soft batches, while transparency draws over a read-only depth attachment.
    scene_depth: Option<wgpu::BindGroup>,

    // Uploaded images keyed by asset identity, and the white texel untextured batches bind (with its asset, which keeps
    // the upload alive).
    textures: HashMap<u64, GpuTexture>,
    white: TextureAsset,

    // The instance buffer, the GPU sort that fills it where compute runs, and this frame's layout of it.
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    sorter: Option<ParticleSorter>,
    prepared: Vec<PreparedBatch>,

    // Scratch kept between frames: instances in upload order, their batches for the GPU sort, and one CPU sort key per
    // instance.
    upload: Vec<ParticleInstance>,
    segments: Vec<u32>,
    order: Vec<(f32, u32)>,
}

impl ParticlePass {
    /// Compile the particle pipelines for `scene_format` from `common` (the scene contract and shadow receivers the
    /// renderer composes for this device). The pipeline layout is `leading_layouts` followed by the sprite texture
    /// group, and for soft pipelines the scene-depth group after it. Devices with compute shaders also get the GPU
    /// sort.
    pub fn new(
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        leading_layouts: &[&wgpu::BindGroupLayout],
        common: &str,
        scene_format: wgpu::TextureFormat,
    ) -> Self {
        let device = gpu.device();
        let capabilities = gpu.capabilities();
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle sprite"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let mut layouts: Vec<&wgpu::BindGroupLayout> = leading_layouts.to_vec();
        layouts.push(&texture_layout);
        let hard = blend_pipelines(device, &layouts, &particle_module_source(common, false), scene_format);

        // WebGL 2 shaders cannot read the depth buffer their pass tests against, so soft particles draw hard there.
        let soft = (capabilities.backend != GpuBackend::Gl).then(|| {
            let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("particle scene depth"),
                entries: &[scene_depth_entry(0)],
            });
            let mut soft_layouts = layouts.clone();
            soft_layouts.push(&depth_layout);
            SoftPipelines {
                pipelines: blend_pipelines(device, &soft_layouts, &particle_module_source(common, true), scene_format),
                depth_layout,
            }
        });
        let sorter = capabilities.compute.then(|| ParticleSorter::new(gpu));

        let white = TextureAsset::from_rgba8(1, 1, vec![255; 4]).expect("one white texel is a valid image");
        let storage = sorter.is_some();
        let mut pass = Self {
            hard,
            soft,
            texture_layout,
            texture_group: leading_layouts.len() as u32,
            sampler: mipmapped_sampler(device, "particle sprite", wgpu::AddressMode::ClampToEdge),
            scene_depth: None,
            textures: HashMap::new(),
            white: white.clone(),
            instance_buffer: instance_buffer(device, INITIAL_INSTANCE_CAPACITY, storage),
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            sorter,
            prepared: Vec::new(),
            upload: Vec::new(),
            segments: Vec::new(),
            order: Vec::new(),
        };
        pass.upload_texture(gpu, mipmaps, &white);
        pass
    }

    /// Cull, order, and upload `frame` for `view`, replacing the previous frame's batches; `None` draws nothing. On
    /// devices with compute shaders the ordering is recorded into `encoder`, ahead of the passes that draw.
    ///
    /// Fails before uploading anything when an image exceeds the device's largest 2D texture.
    pub fn prepare(
        &mut self,
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        encoder: &mut wgpu::CommandEncoder,
        frame: Option<&ParticleFrame>,
        view: &ParticleView,
    ) -> Result<PreparedCounts, RenderError> {
        self.prepared.clear();
        self.upload.clear();
        self.segments.clear();
        self.scene_depth = None;
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

        // Lay out the surviving batches. The GPU sort takes them unsorted, each instance tagged with its batch; without
        // it, source-over batches sort here.
        let gpu_sort = self.sorter.is_some();
        let mut needs_gpu_sort = false;
        let mut counts = PreparedCounts::default();
        for batch in frame.batches() {
            if bounds_outside_view(batch.bounds(), view.view_projection) {
                counts.culled_batches += 1;
                continue;
            }
            let range = batch.instances();
            let instances = &frame.instances()[range.start as usize..range.end as usize];
            let start = self.upload.len() as u32;
            if batch.is_additive() || gpu_sort {
                self.upload.extend_from_slice(instances);
            } else {
                order_back_to_front(instances, view, &mut self.order, &mut self.upload);
            }
            if gpu_sort {
                let segment = (self.prepared.len() as u32) | if batch.is_additive() { UNSORTED_SEGMENT } else { 0 };
                self.segments.extend(std::iter::repeat_n(segment, instances.len()));
                needs_gpu_sort |= !batch.is_additive() && instances.len() > 1;
            }

            let texture = batch.texture().map(|slot| {
                let image = &frame.textures()[slot as usize];
                self.upload_texture(gpu, mipmaps, image);
                image.id()
            });
            self.prepared.push(PreparedBatch {
                instances: start..(self.upload.len() as u32),
                depth: view.depth(batch.sort_point()),
                bounds_depth: view.depth(batch.bounds().center()),
                additive: batch.is_additive(),
                texture,
                soft: batch.is_soft(),
            });
        }
        counts.instances = self.upload.len();

        // The owner's queue keeps equal depths in batch order, so batches sharing a sort point (a presentation's layer)
        // are put back to front among themselves by their bounds here, as batches without one already are. The sort is
        // stable and moves only the batch list, never the instances it points into.
        self.prepared.sort_by(|left, right| {
            right.depth.total_cmp(&left.depth).then(right.bounds_depth.total_cmp(&left.bounds_depth))
        });

        // Fill the instance buffer: sorted on the GPU when some batch needs it, otherwise uploaded as laid out.
        self.ensure_instance_capacity(gpu);
        match &mut self.sorter {
            Some(sorter) if needs_gpu_sort => {
                sorter.encode(gpu, encoder, &self.upload, &self.segments, view, &self.instance_buffer);
            }
            _ => self.write_instances(gpu),
        }
        Ok(counts)
    }

    /// This frame's batches after [`prepare`](Self::prepare), in frame order.
    pub fn batches(&self) -> &[PreparedBatch] {
        &self.prepared
    }

    /// Whether this frame has soft batches and the device can fade them against the scene depth, so transparency must
    /// draw in its own pass over a read-only depth attachment ([`bind_scene_depth`](Self::bind_scene_depth)).
    pub fn reads_scene_depth(&self) -> bool {
        self.soft.is_some() && self.prepared.iter().any(|batch| batch.soft)
    }

    /// Bind `depth` (the opaque scene depth, attached read-only while transparency draws) for this frame's soft
    /// batches, or `None` to draw them hard. Call after [`prepare`](Self::prepare) and before the transparency pass.
    pub fn bind_scene_depth(&mut self, device: &wgpu::Device, depth: Option<&wgpu::TextureView>) {
        self.scene_depth = match (&self.soft, depth) {
            (Some(soft), Some(view)) => Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("particle scene depth"),
                layout: &soft.depth_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                }],
            })),
            _ => None,
        };
    }

    /// Record prepared batch `index` into the open pass, which must already bind the groups before the sprite group.
    /// Soft batches fade against the scene depth when it is bound this frame and draw hard otherwise. Returns the
    /// instances drawn (zero for a missing batch or image).
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
        let soft = match (&self.soft, &self.scene_depth) {
            (Some(soft), Some(depth)) if batch.soft => Some((&soft.pipelines, depth)),
            _ => None,
        };
        let pipelines = soft.map_or(&self.hard, |(pipelines, _)| pipelines);
        pass.set_pipeline(if batch.additive { &pipelines.additive } else { &pipelines.over });
        pass.set_bind_group(self.texture_group, group, &[]);
        if let Some((_, depth)) = soft {
            pass.set_bind_group(self.texture_group + 1, depth, &[]);
        }
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

    /// Grow the instance buffer geometrically to hold this frame's instances.
    fn ensure_instance_capacity(&mut self, gpu: &GpuContext) {
        if self.upload.len() > self.instance_capacity {
            self.instance_capacity = self.upload.len().next_power_of_two();
            self.instance_buffer = instance_buffer(gpu.device(), self.instance_capacity, self.sorter.is_some());
        }
    }

    /// Upload this frame's instances as laid out, already in drawing order.
    fn write_instances(&self, gpu: &GpuContext) {
        if !self.upload.is_empty() {
            gpu.queue().write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.upload));
        }
    }
}

/// Append `instances` to `output` back to front for `view`: farthest first, equal depths in generation order, so the
/// order is deterministic. `order` is scratch (one key per instance). The GPU sort reproduces this order exactly for
/// the same depths.
pub(super) fn order_back_to_front(
    instances: &[ParticleInstance],
    view: &ParticleView,
    order: &mut Vec<(f32, u32)>,
    output: &mut Vec<ParticleInstance>,
) {
    order.clear();
    order.extend(instances.iter().enumerate().map(|(index, instance)| (view.depth(instance.position()), index as u32)));
    order.sort_unstable_by(|left, right| right.0.total_cmp(&left.0).then(left.1.cmp(&right.1)));
    output.extend(order.iter().map(|(_, index)| instances[*index as usize]));
}

/// The particle module for one pipeline family: the scene contract, the shape library, the soft fade (reading the scene
/// depth when `soft`) or its hard stand-in, and the scene entry points.
pub(crate) fn particle_module_source(common: &str, soft: bool) -> String {
    let fade = if soft { PARTICLE_DEPTH_SOURCE } else { PARTICLE_DEPTH_NONE_SOURCE };
    format!("{common}\n{PARTICLE_SHAPES_SOURCE}\n{fade}\n{PARTICLE_SCENE_SOURCE}")
}

/// Compile `source`'s source-over and additive pipelines with `layouts`.
fn blend_pipelines(
    device: &wgpu::Device,
    layouts: &[&wgpu::BindGroupLayout],
    source: &str,
    scene_format: wgpu::TextureFormat,
) -> BlendPipelines {
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().copied().map(Some).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("particles"),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("particles"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    BlendPipelines {
        over: particle_pipeline(
            device,
            &layout,
            &module,
            "fragmentOver",
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            scene_format,
        ),
        additive: particle_pipeline(device, &layout, &module, "fragmentAdditive", ADDITIVE_BLENDING, scene_format),
    }
}

/// The scene depth soft particles read: a depth texture for the fragment stage, loaded without a sampler.
fn scene_depth_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
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

/// The instance buffer for `capacity` instances; `storage` lets the GPU sort write it.
fn instance_buffer(device: &wgpu::Device, capacity: usize, storage: bool) -> wgpu::Buffer {
    let mut usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
    if storage {
        usage |= wgpu::BufferUsages::STORAGE;
    }
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle instances"),
        size: (capacity * std::mem::size_of::<ParticleInstance>()) as u64,
        usage,
        mapped_at_creation: false,
    })
}
