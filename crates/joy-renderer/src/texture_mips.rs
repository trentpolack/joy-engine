// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Mipmapped image uploads for sampled textures (material maps and sprite textures).
//!
//! Without mips, a texture drawn smaller than its size skips texels and shimmers as the camera moves. Each uploaded
//! image gets a full chain down to one texel, generated on the GPU: every level is the previous one resampled at half
//! size with linear filtering. sRGB textures are filtered through sRGB views, so levels average linear light rather than
//! encoded values. Generation is recorded into its own command buffer and submitted with the upload.

use std::collections::HashMap;

use crate::gpu::GpuContext;
use crate::pipelines::{
    sampler_entry,
    texture_entry,
};
use crate::texture::TextureAsset;

const MIPMAP_SOURCE: &str = include_str!("shaders/mipmap.wgsl");

/// Most anisotropic samples taken by trilinear samplers of mipmapped textures; every backend accepts this many.
pub(crate) const MAX_ANISOTROPY: u16 = 8;

/// Number of levels in a full chain for a `width` by `height` image: one more than the largest power of two halving
/// the longer side to one texel.
pub(crate) fn mip_level_count(width: u32, height: u32) -> u32 {
    u32::BITS - width.max(height).max(1).leading_zeros()
}

/// Uploads images with generated mip chains. Owns one downsampling pipeline per texture format.
pub(crate) struct MipmapGenerator {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl MipmapGenerator {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mipmap"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mipmap"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mipmap"),
            source: wgpu::ShaderSource::Wgsl(MIPMAP_SOURCE.into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mipmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { layout, pipeline_layout, module, sampler, pipelines: HashMap::new() }
    }

    /// Upload `asset` as a `format` texture (RGBA8, sRGB or not) with a full mip chain and return it.
    pub(crate) fn upload(
        &mut self,
        gpu: &GpuContext,
        label: &str,
        asset: &TextureAsset,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        let device = gpu.device();
        let level_count = mip_level_count(asset.width(), asset.height());
        let size = wgpu::Extent3d { width: asset.width(), height: asset.height(), depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        // Only level zero comes from the image; the rest are rendered from it.
        gpu.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            asset.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(asset.width() * 4),
                rows_per_image: Some(asset.height()),
            },
            size,
        );
        if level_count > 1 {
            self.generate(gpu, &texture, format, level_count);
        }
        texture
    }

    /// Render levels 1 and up of `texture` from the level above each.
    fn generate(&mut self, gpu: &GpuContext, texture: &wgpu::Texture, format: wgpu::TextureFormat, level_count: u32) {
        let device = gpu.device();
        let pipeline = self.pipelines.entry(format).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mipmap"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.module,
                    entry_point: Some("vertexMain"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
                    entry_point: Some("fragmentMain"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });

        // One view per level: each pass reads the level above (the backend restricts sampling to that level) and writes
        // its own, so no pass reads what it writes.
        let views: Vec<wgpu::TextureView> = (0..level_count)
            .map(|level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("mip level"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mipmaps") });
        for level in 1..level_count as usize {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mipmap"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[level - 1]),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mip level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &views[level],
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue().submit([encoder.finish()]);
    }
}

/// A repeating or clamping trilinear, anisotropic sampler for mipmapped textures.
pub(crate) fn mipmapped_sampler(device: &wgpu::Device, label: &str, address: wgpu::AddressMode) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: MAX_ANISOTROPY,
        ..Default::default()
    })
}
