// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene-effect attachments: view-space normals with roughness, and screen-space motion with and without temporal
//! jitter. Opaque materials write all three beside the scene color in the scene pass itself (see `SceneTargets` in the
//! materials module), so opaque geometry is rasterized once whatever the effects need.

use crate::materials::ATTACHMENT_FORMAT;

/// One owned attachment texture and its view.
struct Attachment {
    // The view references the texture; drop it first.
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
}

impl Attachment {
    fn new(device: &wgpu::Device, label: &str, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATTACHMENT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self { view: texture.create_view(&wgpu::TextureViewDescriptor::default()), _texture: texture }
    }

    /// A color attachment cleared to `clear` and stored for the effects.
    fn color_attachment(&self, clear: wgpu::Color) -> wgpu::RenderPassColorAttachment<'_> {
        wgpu::RenderPassColorAttachment {
            view: &self.view,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            depth_slice: None,
        }
    }
}

/// The normal-and-roughness and motion targets the scene pass writes for scene effects, matched to the frame size.
pub(crate) struct SceneAttachments {
    normal_roughness: Attachment,
    motion: Attachment,
    jittered_motion: Attachment,
    size: (u32, u32),
}

impl SceneAttachments {
    /// Allocate attachments for a `size` frame in physical pixels.
    pub(crate) fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        Self {
            normal_roughness: Attachment::new(device, "scene normal roughness", size),
            motion: Attachment::new(device, "scene motion", size),
            jittered_motion: Attachment::new(device, "scene motion jittered", size),
            size,
        }
    }

    /// The frame size these attachments were allocated for.
    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The scene pass's attachment slots after its color target, in material output order. Uncovered pixels face the
    /// camera and do not move.
    pub(crate) fn color_attachments(&self) -> [Option<wgpu::RenderPassColorAttachment<'_>>; 3] {
        let facing_camera = wgpu::Color { r: 0.5, g: 0.5, b: 1.0, a: 1.0 };
        [
            Some(self.normal_roughness.color_attachment(facing_camera)),
            Some(self.motion.color_attachment(wgpu::Color::TRANSPARENT)),
            Some(self.jittered_motion.color_attachment(wgpu::Color::TRANSPARENT)),
        ]
    }

    /// View-space normals with roughness in alpha.
    pub(crate) fn normal_roughness(&self) -> &wgpu::TextureView {
        &self.normal_roughness.view
    }

    /// Unjittered motion (antialiasing and motion blur).
    pub(crate) fn motion(&self) -> &wgpu::TextureView {
        &self.motion.view
    }

    /// Jittered motion (lighting effects).
    pub(crate) fn jittered_motion(&self) -> &wgpu::TextureView {
        &self.jittered_motion.view
    }
}
