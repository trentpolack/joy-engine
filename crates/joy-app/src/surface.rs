// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Presentation surface configuration.
//!
//! The renderer's presentation pass encodes sRGB itself and egui paints gamma-space colors, so both write the encoded
//! bytes a display expects through a non-sRGB view. When a platform only offers sRGB swapchain formats, the surface
//! keeps the sRGB format and exposes a non-sRGB view format of the same texture.

use joy_renderer::{
    GpuContext,
    RenderError,
};

/// A configured surface and the view format the renderer presents into.
pub(crate) struct ConfiguredSurface {
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub view_format: wgpu::TextureFormat,
}

impl ConfiguredSurface {
    /// Choose formats for `surface` on the context's adapter and configure it at `width` by `height` pixels.
    pub fn new(
        gpu: &GpuContext,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, RenderError> {
        let capabilities = surface.get_capabilities(gpu.adapter());
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| RenderError::Surface("the surface reports no formats".into()))?;
        let view_format = format.remove_srgb_suffix();
        let mut config = surface
            .get_default_config(gpu.adapter(), width.max(1), height.max(1))
            .ok_or_else(|| RenderError::Surface("the surface is not supported by this adapter".into()))?;
        config.format = format;
        config.view_formats = if view_format == format { Vec::new() } else { vec![view_format] };
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(gpu.device(), &config);
        Ok(Self { surface, config, view_format })
    }

    /// Reconfigure for a new drawing size; zero sizes are ignored (minimized windows).
    pub fn resize(&mut self, gpu: &GpuContext, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(gpu.device(), &self.config);
    }

    /// Reapply the current configuration after the surface reports it is outdated or lost.
    pub fn reconfigure(&self, gpu: &GpuContext) {
        self.surface.configure(gpu.device(), &self.config);
    }
}
