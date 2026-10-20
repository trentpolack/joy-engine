// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! What every scene effect reads about the frame (the scene targets, this frame's cameras, the previous frame's
//! camera, and the history clock), and the target helpers the effect owners allocate with.

use joy_core::math::Mat4;

use crate::camera::CameraFrame;

/// One frame's scene targets and cameras, as the renderer hands them to the effects.
pub(crate) struct EffectsFrame<'a> {
    /// Render size in physical pixels.
    pub size: (u32, u32),
    /// The HDR scene color: effects read it and copy their results back into it.
    pub scene_view: &'a wgpu::TextureView,
    pub scene_texture: &'a wgpu::Texture,
    /// The scene depth after the opaque pass.
    pub depth_view: &'a wgpu::TextureView,
    /// The camera as supplied, without jitter.
    pub camera: CameraFrame,
    /// The jittered projection geometry was rasterized with.
    pub raster_projection: Mat4,
    /// The previous frame's unjittered view-projection and view; this frame's without history.
    pub previous_view_projection: Mat4,
    pub previous_view: Mat4,
    /// Presentation seconds since the previous frame; zero without history.
    pub delta_seconds: f32,
    /// Whether history restarts this frame (the first frame, a cut, a clock jump, or an explicit reset).
    pub reset_history: bool,
}

impl EffectsFrame<'_> {
    /// The previous frame's view-projection times this frame's inverse, both unjittered: it carries a far-plane point
    /// (the sky, which writes no motion) to where the previous frame saw it.
    pub(crate) fn sky_reprojection(&self) -> Mat4 {
        self.previous_view_projection * self.camera.view_projection().inverse()
    }
}

/// A single-sample 2D effect target of `size` physical pixels.
pub(crate) fn effect_target(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// A bilinear sampler clamped to the edge, for reading effect targets and volumes.
pub(crate) fn linear_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

/// Copy a finished effect target (the scene's size and format) back into the scene color, so later effects and the
/// display chain read one texture whichever effects ran.
pub(crate) fn copy_into_scene(encoder: &mut wgpu::CommandEncoder, source: &wgpu::Texture, frame: &EffectsFrame<'_>) {
    encoder.copy_texture_to_texture(
        source.as_image_copy(),
        frame.scene_texture.as_image_copy(),
        wgpu::Extent3d {
            width: frame.size.0.max(1),
            height: frame.size.1.max(1),
            depth_or_array_layers: 1,
        },
    );
}
