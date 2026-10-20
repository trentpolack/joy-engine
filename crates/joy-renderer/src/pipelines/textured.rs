// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The textured transparent material for [`FrameInputs::transparent_draws`](crate::FrameInputs).
//!
//! One blended pipeline samples a sprite texture and multiplies it by vertex color, depth-tested (`LessEqual`) against
//! opaque geometry without writing depth. Textured triangles use this pass's own vertex buffer, written in the
//! transparent queue's drawing order. Textures upload with mip chains (decoded from sRGB) on first use and release once
//! their [`TextureAsset`] is dropped everywhere.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Weak;

use crate::RenderError;
use crate::frame::{
    TexturedVertex,
    TransparentDraw,
};
use crate::gpu::GpuContext;
use crate::texture::TextureAsset;
use crate::texture_mips::{
    MipmapGenerator,
    mipmapped_sampler,
};

use super::{
    DEPTH_FORMAT,
    sampler_entry,
    scene_common,
    texture_entry,
};

/// The textured material's entry points, appended to the shared scene contract.
const TEXTURED_SOURCE: &str = include_str!("../shaders/textured.wgsl");

/// Vertex attributes of [`TexturedVertex`]: position, color, and UV at locations 0 through 2.
const TEXTURED_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32x2];

/// Initial textured vertex capacity; grows geometrically.
const INITIAL_TEXTURED_CAPACITY: usize = 1024;

/// Sprite texture format; sampling decodes sRGB to linear.
const SPRITE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// GPU copy of one texture, released when the asset is dropped everywhere.
struct GpuTexture {
    // The bind group references the view, which references the texture; drop in that order.
    bind_group: wgpu::BindGroup,
    _view: wgpu::TextureView,
    _texture: wgpu::Texture,
    asset: Weak<dyn std::any::Any + Send + Sync>,
}

/// Owned textured-material resources. Borrows the device, frame uniforms, and the open scene pass per frame.
pub(crate) struct TexturedPass {
    pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    textures: HashMap<u64, GpuTexture>,
}

impl TexturedPass {
    /// Compile the textured pipeline for the scene format. The layout is frame uniforms, the (unused) shadow receiver,
    /// then the sprite texture, so the scene pass's groups 0 and 1 stay bound across material switches.
    pub fn new(
        device: &wgpu::Device,
        frame_layout: &wgpu::BindGroupLayout,
        shadow_layout: &wgpu::BindGroupLayout,
        scene_format: wgpu::TextureFormat,
    ) -> Self {
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("textured sprite"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("textured"),
            bind_group_layouts: &[Some(frame_layout), Some(shadow_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let source = format!("{}\n{TEXTURED_SOURCE}", scene_common(device));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("textured"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("textured"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertexMain"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TexturedVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &TEXTURED_ATTRIBUTES,
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
                module: &module,
                entry_point: Some("fragmentMain"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: scene_format,
                    // Color: src-alpha / one-minus-src-alpha; alpha: one / one-minus-src-alpha.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = mipmapped_sampler(device, "textured sprite", wgpu::AddressMode::ClampToEdge);
        let vertex_buffer = textured_vertex_buffer(device, INITIAL_TEXTURED_CAPACITY);
        Self {
            pipeline,
            texture_layout,
            sampler,
            vertex_buffer,
            vertex_capacity: INITIAL_TEXTURED_CAPACITY,
            textures: HashMap::new(),
        }
    }

    /// Upload every texture `draws` references (with mip chains) the first time it appears, before any pass opens.
    ///
    /// Fails without uploading when a texture exceeds the device's largest 2D texture.
    pub fn upload_textures(
        &mut self,
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        draws: &[TransparentDraw<'_>],
    ) -> Result<(), RenderError> {
        let limit = gpu.capabilities().max_texture_dimension;
        for draw in draws {
            if let TransparentDraw::Textured {
                texture,
                ..
            } = draw
                && (texture.width() > limit || texture.height() > limit)
            {
                return Err(RenderError::InvalidFrame(format!(
                    "a {}x{} texture exceeds this device's {limit}-pixel limit",
                    texture.width(),
                    texture.height()
                )));
            }
        }
        for draw in draws {
            if let TransparentDraw::Textured {
                texture,
                ..
            } = draw
            {
                self.upload_texture(gpu, mipmaps, texture);
            }
        }
        Ok(())
    }

    /// Record one textured run into the open scene pass; returns whether the texture was available.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, texture: u64, vertices: Range<u32>) -> bool {
        let Some(gpu_texture) = self.textures.get(&texture) else {
            return false;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(2, &gpu_texture.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(vertices, 0..1);
        true
    }

    /// Release uploads whose textures no longer exist anywhere.
    pub fn release_dropped(&mut self) {
        self.textures.retain(|_, texture| texture.asset.strong_count() > 0);
    }

    /// Upload a texture with its mip chain the first time it is drawn.
    fn upload_texture(&mut self, gpu: &GpuContext, mipmaps: &mut MipmapGenerator, asset: &TextureAsset) {
        if self.textures.contains_key(&asset.id()) {
            return;
        }
        let device = gpu.device();
        let texture = mipmaps.upload(gpu, "sprite texture", asset, SPRITE_FORMAT);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite texture"),
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

    /// Upload this frame's textured vertices, in drawing order, growing the buffer geometrically.
    pub fn write_vertices(&mut self, gpu: &GpuContext, vertices: &[TexturedVertex]) {
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > self.vertex_capacity {
            self.vertex_capacity = vertices.len().next_power_of_two();
            self.vertex_buffer = textured_vertex_buffer(gpu.device(), self.vertex_capacity);
        }
        gpu.queue().write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(vertices));
    }
}

fn textured_vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("textured triangles"),
        size: (capacity * std::mem::size_of::<TexturedVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
