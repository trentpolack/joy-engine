// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The deferred lighting G-buffer: one `Rgba32Uint` texel per pixel holding the engine's header word (shading normal
//! and shadow flag) and three words a shading model encodes (gbuffer-encoding.wgsl), matched to the render size.

use crate::pipelines::GBUFFER_FORMAT;

/// The G-buffer for one render size.
pub(crate) struct GBuffer {
    // The view references the texture; drop it first.
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    size: (u32, u32),
}

impl GBuffer {
    /// Allocate the G-buffer for a `size` frame in physical pixels.
    pub(crate) fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("g-buffer"),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GBUFFER_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            _texture: texture,
            size,
        }
    }

    /// The frame size this G-buffer was allocated for.
    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The G-buffer attachment, cleared at the start of the G-buffer pass.
    pub(crate) fn color_attachment(&self) -> Option<wgpu::RenderPassColorAttachment<'_>> {
        Some(wgpu::RenderPassColorAttachment {
            view: &self.view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })
    }

    /// The view the lighting passes read.
    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}
