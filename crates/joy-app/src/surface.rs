// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Presentation surface configuration.
//!
//! The renderer's presentation pass encodes sRGB itself and egui paints gamma-space colors, so both write the encoded
//! bytes a display expects through a non-sRGB view. When a platform only offers sRGB swapchain formats, the surface
//! keeps the sRGB format and exposes a non-sRGB view format of the same texture.
//!
//! With high dynamic range allowed, a display that reports it can show HDR gets a half-float surface with an
//! extended-range color space instead: linear scRGB natively (Metal, DirectX 12, and Vulkan all offer it, and it is the
//! only HDR form DirectX 12 has) and extended sRGB in browsers (WebGPU has no linear canvas). Every other display keeps
//! the standard surface, which costs half the bandwidth. The choice is made once, at startup; the headroom
//! and SDR white level it reports are read again while running (see [`ConfiguredSurface::display_output`]).

use joy_renderer::{
    DisplayOutput,
    GpuContext,
    OutputEncoding,
    RenderError,
};

/// Headroom assumed for an HDR display that reports none (browsers expose only whether a display is HDR-capable): a
/// conservative multiple of SDR white that every HDR display reaches.
const UNKNOWN_HDR_HEADROOM: f32 = 2.0;

/// Luminance of scRGB `1.0` on Windows, in nits; SDR white sits at the user's SDR content brightness above it.
const SCRGB_REFERENCE_WHITE_NITS: f32 = 80.0;

/// The surface format extended-range output uses on every platform.
const EXTENDED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Extended-range color spaces in order of preference, with the signal each expects.
const EXTENDED_COLOR_SPACES: [(wgpu::SurfaceColorSpace, OutputEncoding); 2] = [
    (wgpu::SurfaceColorSpace::ExtendedSrgbLinear, OutputEncoding::ExtendedLinear),
    (wgpu::SurfaceColorSpace::ExtendedSrgb, OutputEncoding::ExtendedSrgb),
];

/// A configured surface, the view format the renderer presents into, and the signal it expects.
pub(crate) struct ConfiguredSurface {
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub view_format: wgpu::TextureFormat,
    /// What the renderer and UI write: standard sRGB, or an extended-range signal.
    pub encoding: OutputEncoding,
}

impl ConfiguredSurface {
    /// Choose formats for `surface` on the context's adapter and configure it at `width` by `height` pixels. With
    /// `high_dynamic_range`, an extended-range surface is preferred when the display reports HDR and the surface
    /// offers one.
    pub fn new(
        gpu: &GpuContext,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        high_dynamic_range: bool,
    ) -> Result<Self, RenderError> {
        let capabilities = surface.get_capabilities(gpu.adapter());
        let config = surface
            .get_default_config(gpu.adapter(), width.max(1), height.max(1))
            .ok_or_else(|| RenderError::Surface("the surface is not supported by this adapter".into()))?;
        let mut configured = Self {
            surface,
            config,
            view_format: wgpu::TextureFormat::Rgba8Unorm,
            encoding: OutputEncoding::Standard,
        };

        let extended = (high_dynamic_range && display_shows_hdr(&configured.surface.display_hdr_info(gpu.adapter())))
            .then(|| extended_color_space(&capabilities))
            .flatten();
        match extended {
            Some((color_space, encoding)) => configured.use_extended_range(gpu, color_space, encoding),
            None => configured.use_standard_range(gpu, &capabilities)?,
        }
        Ok(configured)
    }

    /// Reconfigure as a standard sRGB surface, for platforms that accepted an extended-range configuration but will not
    /// present it (browsers without HDR canvases).
    #[cfg(target_arch = "wasm32")]
    pub fn fall_back_to_standard_range(&mut self, gpu: &GpuContext) -> Result<(), RenderError> {
        let capabilities = self.surface.get_capabilities(gpu.adapter());
        self.use_standard_range(gpu, &capabilities)
    }

    /// The display's current signal: the configured encoding, its headroom, and its SDR white level. Re-queries the
    /// operating system, so the shell calls it occasionally rather than every frame.
    pub fn display_output(&self, gpu: &GpuContext) -> DisplayOutput {
        if !self.encoding.is_extended() {
            return DisplayOutput::STANDARD;
        }
        let info = self.surface.display_hdr_info(gpu.adapter());
        let headroom = info.tone_map_headroom().unwrap_or(UNKNOWN_HDR_HEADROOM);
        let sdr_white_level = info
            .luminance
            .and_then(|luminance| luminance.sdr_white_nits)
            .filter(|nits| nits.is_finite() && *nits > 0.0)
            .map_or(1.0, |nits| nits / SCRGB_REFERENCE_WHITE_NITS);
        DisplayOutput::extended(self.encoding, headroom, sdr_white_level)
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

    /// Configure the half-float surface in an extended-range color space.
    fn use_extended_range(&mut self, gpu: &GpuContext, color_space: wgpu::SurfaceColorSpace, encoding: OutputEncoding) {
        self.config.format = EXTENDED_FORMAT;
        self.config.color_space = color_space;
        self.config.view_formats = Vec::new();
        self.view_format = EXTENDED_FORMAT;
        self.encoding = encoding;
        self.configure(gpu);
    }

    /// Configure the first non-sRGB format (or an sRGB format with a non-sRGB view) in the default color space.
    fn use_standard_range(
        &mut self,
        gpu: &GpuContext,
        capabilities: &wgpu::SurfaceCapabilities,
    ) -> Result<(), RenderError> {
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb() && *format != EXTENDED_FORMAT)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| RenderError::Surface("the surface reports no formats".into()))?;
        let view_format = format.remove_srgb_suffix();
        self.config.format = format;
        self.config.color_space = wgpu::SurfaceColorSpace::Auto;
        self.config.view_formats = if view_format == format { Vec::new() } else { vec![view_format] };
        self.view_format = view_format;
        self.encoding = OutputEncoding::Standard;
        self.configure(gpu);
        Ok(())
    }

    /// Apply the configuration with the shell's presentation mode.
    fn configure(&mut self, gpu: &GpuContext) {
        self.config.present_mode = wgpu::PresentMode::AutoVsync;
        self.surface.configure(gpu.device(), &self.config);
    }
}

/// Whether a display reports that it can show HDR: browsers and Windows through their dynamic-range flag, Apple displays
/// through potential headroom above SDR white, and platforms with absolute luminance through a peak above SDR white.
/// Displays that report nothing (Vulkan outside Windows, for example) keep standard output: an SDR compositor clamps an
/// extended surface, which would clip highlights that standard tone mapping keeps.
fn display_shows_hdr(info: &wgpu::DisplayHdrInfo) -> bool {
    let flag = info.coarse.and_then(|coarse| coarse.high_dynamic_range);
    if flag == Some(false) {
        return false;
    }
    let potential_headroom =
        info.headroom.and_then(|headroom| headroom.potential).is_some_and(|potential| potential > 1.0);
    let measured_headroom = info.tone_map_headroom().is_some_and(|headroom| headroom > 1.0);
    flag == Some(true) || potential_headroom || measured_headroom
}

/// The preferred extended-range color space the surface offers on the half-float format, with the signal it expects.
fn extended_color_space(capabilities: &wgpu::SurfaceCapabilities) -> Option<(wgpu::SurfaceColorSpace, OutputEncoding)> {
    let supported = capabilities.color_spaces(EXTENDED_FORMAT);
    EXTENDED_COLOR_SPACES
        .into_iter()
        .find(|(color_space, _)| color_space.to_color_spaces().is_some_and(|flags| supported.contains(flags)))
}
