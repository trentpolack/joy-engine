// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The scene-effect attachment: screen-space motion, view depth, and coverage, which opaque surfaces write beside the
//! scene color in the scene pass itself (see `SceneTargets` in the pipelines module and scene-common.wgsl), so opaque
//! geometry is rasterized once and effects never load the depth buffer (WebGL 2 shaders cannot).

use crate::pipelines::ATTACHMENT_FORMAT;

/// The motion target the scene pass writes for temporal antialiasing and motion blur, matched to the frame size.
pub(crate) struct SceneAttachments {
    // The view references the texture; drop it first.
    motion: wgpu::TextureView,
    _texture: wgpu::Texture,
    size: (u32, u32),
}

impl SceneAttachments {
    /// Allocate the attachment for a `size` frame in physical pixels.
    pub(crate) fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene motion"),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATTACHMENT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            motion: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            _texture: texture,
            size,
        }
    }

    /// The frame size this attachment was allocated for.
    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The attachment slots after a scene pass's color target. The first pass of a frame clears them (uncovered pixels,
    /// such as the sky, keep zero coverage); a later pass over the same frame loads what earlier passes wrote.
    pub(crate) fn color_attachments(&self, clear: bool) -> [Option<wgpu::RenderPassColorAttachment<'_>>; 1] {
        let load = if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load };
        [Some(wgpu::RenderPassColorAttachment {
            view: &self.motion,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })]
    }

    /// RG screen-space motion in UV units from the previous frame's position to this one's (without jitter), B view
    /// depth, A coverage.
    pub(crate) fn motion(&self) -> &wgpu::TextureView {
        &self.motion
    }
}
