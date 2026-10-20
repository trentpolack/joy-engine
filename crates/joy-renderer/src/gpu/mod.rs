// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU instance, adapter, and device ownership with backend selection and capability detection.
//!
//! Browser builds prefer WebGPU and fall back to WebGL 2 when WebGPU is missing. Native builds use Vulkan, Metal, or
//! DirectX 12, with OpenGL as a fallback. Everything that
//! depends on the backend (limits, the HDR scene format) is decided here once, from the created device.

use std::sync::Arc;

use crate::RenderError;

/// Preferred HDR scene format when the adapter can render, filter, and blend it.
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Fallback scene format with reduced highlight range, for adapters that cannot render, filter, and blend half floats.
const LDR_SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Color targets the deferred G-buffer pass writes at most: scene color, motion, and G-buffer.
const DEFERRED_COLOR_TARGETS: u32 = 3;

/// Bytes per pixel across those targets: 8 (`Rgba16Float`) + 8 (`Rgba16Float`) + 16 (`Rgba32Uint`), the WebGPU default
/// limit.
const DEFERRED_BYTES_PER_SAMPLE: u32 = 32;

/// Which family of graphics APIs to request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendPreference {
    /// WebGPU in browsers (falling back to WebGL 2), or the platform's primary native API.
    #[default]
    Auto,
    /// Force WebGPU in browsers, or the platform's primary native API (no OpenGL fallback).
    Primary,
    /// Force WebGL 2 in browsers, or OpenGL natively. Useful for testing the fallback path.
    WebGl,
}

impl BackendPreference {
    /// Parse the browser's `?backend=` query values (`webgpu`, `webgl`); anything else selects `Auto`.
    pub fn from_query_value(value: Option<&str>) -> Self {
        match value {
            Some("webgpu") => Self::Primary,
            Some("webgl") => Self::WebGl,
            _ => Self::Auto,
        }
    }

    /// wgpu backends requested for this preference on the current target.
    pub fn backends(self) -> wgpu::Backends {
        match self {
            Self::Auto if cfg!(target_arch = "wasm32") => wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            Self::Auto => wgpu::Backends::PRIMARY | wgpu::Backends::GL,
            Self::Primary if cfg!(target_arch = "wasm32") => wgpu::Backends::BROWSER_WEBGPU,
            Self::Primary => wgpu::Backends::PRIMARY,
            Self::WebGl => wgpu::Backends::GL,
        }
    }
}

/// Graphics API actually selected for the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuBackend {
    /// Browser WebGPU.
    WebGpu,
    /// Browser WebGL 2 or native OpenGL ES / OpenGL.
    Gl,
    /// Native Vulkan.
    Vulkan,
    /// Native Metal.
    Metal,
    /// Native DirectX 12.
    Dx12,
    /// Any other backend (for example, the no-op test backend).
    Other,
}

impl GpuBackend {
    /// Human-readable backend label for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::WebGpu => "WebGPU",
            Self::Gl => {
                if cfg!(target_arch = "wasm32") {
                    "WebGL 2"
                } else {
                    "OpenGL"
                }
            }
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
            Self::Dx12 => "DirectX 12",
            Self::Other => "Other",
        }
    }
}

/// Capabilities resolved once from the created device.
#[derive(Clone, Debug)]
pub struct GpuCapabilities {
    /// Selected graphics API.
    pub backend: GpuBackend,
    /// Adapter name reported by the driver or browser.
    pub adapter_name: String,
    /// Scene color format: `Rgba16Float` when renderable, filterable, and blendable, otherwise `Rgba8Unorm`.
    pub scene_format: wgpu::TextureFormat,
    /// Whether the scene format preserves HDR highlights.
    pub hdr: bool,
    /// Largest supported 2D texture dimension in pixels.
    pub max_texture_dimension: u32,
    /// Whether compute shaders and storage textures are available (every backend but WebGL 2); volumetric fog and the
    /// adaptive-exposure histogram need them.
    pub compute: bool,
    /// Whether GPU timestamp queries are available, so frame profiles include GPU time (not WebGL 2).
    pub timestamps: bool,
    /// Whether deferred lighting can run: every backend but WebGL 2, with an HDR scene format and room for the scene
    /// color, motion, and G-buffer targets in one pass. Without it every surface shades forward.
    pub deferred: bool,
}

/// Shared ownership of one wgpu instance, adapter, device, and queue.
///
/// Cloning is cheap; every renderer, surface, and tool borrowing the same device holds a clone. GPU resources created
/// from the device must be dropped before the last clone.
#[derive(Clone)]
pub struct GpuContext {
    inner: Arc<GpuInner>,
}

/// Field order is drop order: the device and queue are released before the adapter and instance.
struct GpuInner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::Adapter,
    instance: wgpu::Instance,
    capabilities: GpuCapabilities,
}

impl GpuContext {
    /// Create a wgpu instance for the preferred backends.
    ///
    /// In browsers this checks whether WebGPU actually yields an adapter before committing to it, so a browser that
    /// exposes `navigator.gpu` without a usable adapter still falls back to WebGL 2.
    pub async fn create_instance(preference: BackendPreference) -> wgpu::Instance {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = preference.backends();

        #[cfg(target_arch = "wasm32")]
        {
            if descriptor.backends.contains(wgpu::Backends::BROWSER_WEBGPU) {
                return wgpu::util::new_instance_with_webgpu_detection(descriptor).await;
            }
        }
        wgpu::Instance::new(descriptor)
    }

    /// Request an adapter and device from `instance`.
    ///
    /// Pass the presentation surface when one exists; WebGL 2 requires it. Limits are the adapter's limits for native
    /// and WebGPU, and WebGL 2 downlevel defaults (raised to the adapter's resolution limits) for the GL backend.
    pub async fn new(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Self, RenderError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: surface,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| RenderError::NoAdapter(error.to_string()))?;

        let info = adapter.get_info();
        let backend = match info.backend {
            wgpu::Backend::BrowserWebGpu => GpuBackend::WebGpu,
            wgpu::Backend::Gl => GpuBackend::Gl,
            wgpu::Backend::Vulkan => GpuBackend::Vulkan,
            wgpu::Backend::Metal => GpuBackend::Metal,
            wgpu::Backend::Dx12 => GpuBackend::Dx12,
            _ => GpuBackend::Other,
        };

        // WebGL 2 reports conservative limits; start from its defaults so device creation never asks for more.
        let required_limits = if backend == GpuBackend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
        } else {
            adapter.limits()
        };
        // Timestamp queries feed the frame profiler; request them where the adapter offers them.
        let timestamps = backend != GpuBackend::Gl && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let required_features = if timestamps { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("joy-engine"),
                required_features,
                required_limits,
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::DeviceCreation(error.to_string()))?;

        let hdr = supports_hdr_scene(&adapter);
        let compute = backend != GpuBackend::Gl
            && adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
            && device.limits().max_storage_textures_per_shader_stage >= 1;
        let limits = device.limits();
        let deferred = backend != GpuBackend::Gl
            && hdr
            && limits.max_color_attachments >= DEFERRED_COLOR_TARGETS
            && limits.max_color_attachment_bytes_per_sample >= DEFERRED_BYTES_PER_SAMPLE;
        let capabilities = GpuCapabilities {
            backend,
            adapter_name: info.name.clone(),
            scene_format: if hdr { HDR_FORMAT } else { LDR_SCENE_FORMAT },
            hdr,
            max_texture_dimension: limits.max_texture_dimension_2d,
            compute,
            timestamps,
            deferred,
        };
        log::info!("joy-engine: {} on {} (scene format {:?})", backend.label(), info.name, capabilities.scene_format);

        // Browser wgpu objects are single-threaded, so this `Arc` is never shared across threads there; one shared type
        // keeps native contexts `Send + Sync` without a second browser-only code path.
        #[cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]
        let inner = Arc::new(GpuInner { device, queue, adapter, instance, capabilities });
        Ok(Self { inner })
    }

    /// Create a device without a presentation surface, for offscreen rendering, captures, and tests.
    ///
    /// WebGL 2 cannot run headless; browser builds must use [`GpuContext::new`] with a canvas surface.
    pub async fn headless(preference: BackendPreference) -> Result<Self, RenderError> {
        let instance = Self::create_instance(preference).await;
        Self::new(instance, None).await
    }

    /// The device used to create every GPU resource.
    pub fn device(&self) -> &wgpu::Device {
        &self.inner.device
    }

    /// The queue used for uploads and submissions.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.inner.queue
    }

    /// The adapter the device was created from.
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.inner.adapter
    }

    /// The instance that created the adapter; surfaces must come from this instance.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.inner.instance
    }

    /// Capabilities resolved when the device was created.
    pub fn capabilities(&self) -> &GpuCapabilities {
        &self.inner.capabilities
    }
}

/// Whether the adapter can render to, sample with filtering, and blend the HDR scene format.
fn supports_hdr_scene(adapter: &wgpu::Adapter) -> bool {
    let features = adapter.get_texture_format_features(HDR_FORMAT);
    let usages = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let flags = wgpu::TextureFormatFeatureFlags::FILTERABLE | wgpu::TextureFormatFeatureFlags::BLENDABLE;
    features.allowed_usages.contains(usages) && features.flags.contains(flags)
}
