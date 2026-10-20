// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Image-based lighting for PBR surfaces: an equirectangular environment becomes a GGX-prefiltered specular cube with
//! one perceptual roughness per mip (`capture.rs`) and L2 spherical-harmonic diffuse irradiance (`irradiance.rs`), and
//! the renderer owns one split-sum BRDF table that every environment shares.
//!
//! Preparation submits its own GPU work; call it during setup or when the environment changes, never per frame. The
//! intensity and horizontal rotation are per-frame inputs ([`ImageLighting`]) and never regenerate textures.

use std::sync::OnceLock;

use crate::RenderError;
use crate::gpu::GpuContext;
use joy_core::math::deterministic;

use super::capture::{
    CapturePipelines,
    brdf_table,
    cube_texture,
    cube_view,
    upload_source,
};
use super::irradiance::{
    IrradianceCoefficients,
    irradiance_coefficients,
};

/// Equirectangular RGBA pixels, top row first: byte RGB is linear UNORM unless marked sRGB; float RGB is linear HDR.
#[derive(Clone, Copy, Debug)]
pub enum EnvironmentData<'a> {
    /// Eight-bit channels.
    Unorm8(&'a [u8]),
    /// Linear HDR channels, stored as half floats.
    Float(&'a [f32]),
}

/// An equirectangular environment image: U wraps around the vertical axis from +X, V runs from straight up to down.
#[derive(Clone, Copy, Debug)]
pub struct EnvironmentPixels<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four channels per pixel.
    pub data: EnvironmentData<'a>,
}

/// Source color encoding for eight-bit environments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnvironmentEncoding {
    /// Channels are linear.
    #[default]
    Linear,
    /// Channels are sRGB-encoded and decoded while integrating.
    Srgb,
}

/// Integration settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentMapOptions {
    /// Source color encoding.
    pub encoding: EnvironmentEncoding,
    /// Texels per face edge of the specular cube's sharpest (mirror) level; smaller levels hold rougher lobes down to
    /// four texels.
    pub size: u32,
    /// Importance samples per texel for each rough level, 1 to 1024. Filtered sampling keeps 64 free of noise.
    pub sample_count: u32,
}

impl Default for EnvironmentMapOptions {
    fn default() -> Self {
        Self {
            encoding: EnvironmentEncoding::Linear,
            size: 128,
            sample_count: 64,
        }
    }
}

/// Prepared lighting: the specular cube and the irradiance coefficients. Owned by the caller and borrowed per frame
/// through [`ImageLighting`]; drop it before the device.
pub struct EnvironmentMap {
    // The bind group references the views and buffer; drop it first.
    pub(crate) bind_group: wgpu::BindGroup,
    _irradiance: wgpu::Buffer,
    _specular_view: wgpu::TextureView,
    _specular: wgpu::Texture,
    /// Highest specular mip, the roughness-one level.
    pub(crate) max_mip: f32,
}

impl std::fmt::Debug for EnvironmentMap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("EnvironmentMap").field("max_mip", &self.max_mip).finish_non_exhaustive()
    }
}

/// Image-based lighting for one frame.
#[derive(Clone, Copy, Debug)]
pub struct ImageLighting<'a> {
    /// The prepared environment.
    pub map: &'a EnvironmentMap,
    /// Linear radiance multiplier; nonnegative.
    pub intensity: f32,
    /// Horizontal rotation of the environment in radians.
    pub rotation: f32,
}

/// The image-lighting group (group 2 in environment-lighting.wgsl): per-frame parameters, irradiance coefficients, the
/// specular cube, the BRDF table, and one filtering sampler.
pub(crate) fn image_lighting_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float {
                filterable: true,
            },
            view_dimension,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("image lighting"),
        entries: &[
            crate::pipelines::uniform_entry(0),
            crate::pipelines::uniform_entry(1),
            texture(2, wgpu::TextureViewDimension::Cube),
            texture(3, wgpu::TextureViewDimension::D2),
            crate::pipelines::sampler_entry(4),
        ],
    })
}

/// Lighting texture format: half float where it is renderable and filterable (the HDR scene format), else eight-bit.
pub(crate) fn environment_format(scene_format: wgpu::TextureFormat) -> wgpu::TextureFormat {
    if scene_format == wgpu::TextureFormat::Rgba16Float {
        wgpu::TextureFormat::Rgba16Float
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    }
}

/// The renderer's image-lighting state: the per-frame parameters every environment's group binds, the shared BRDF
/// table and sampler, the group bound without an environment (zero irradiance and a black specular cube), and the
/// capture passes, compiled the first time an environment is prepared.
pub(crate) struct ImageLightingResources {
    // Groups reference the buffers and views below; drop them first.
    default_group: wgpu::BindGroup,
    parameters: wgpu::Buffer,
    sampler: wgpu::Sampler,
    brdf_view: wgpu::TextureView,
    _brdf: wgpu::Texture,
    _default_irradiance: wgpu::Buffer,
    _default_specular_view: wgpu::TextureView,
    _default_specular: wgpu::Texture,
    format: wgpu::TextureFormat,
    capture: OnceLock<CapturePipelines>,
}

impl ImageLightingResources {
    /// Create the shared resources for groups of `layout`; integrates the BRDF table on the GPU.
    pub(crate) fn new(gpu: &GpuContext, layout: &wgpu::BindGroupLayout) -> Self {
        let device = gpu.device();
        let format = environment_format(gpu.capabilities().scene_format);
        let parameters = uniform_buffer(device, "image lighting", 16);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image lighting"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let brdf = brdf_table(gpu, format);
        let brdf_view = brdf.create_view(&wgpu::TextureViewDescriptor::default());

        // Without an environment: zero irradiance (buffers start zeroed) and a one-texel black cube.
        let default_irradiance = uniform_buffer(device, "no irradiance", std::mem::size_of::<IrradianceCoefficients>());
        let default_specular = cube_texture(device, "no specular", 1, 1, format, wgpu::TextureUsages::TEXTURE_BINDING);
        let default_specular_view = cube_view(&default_specular, 0, 1);
        let default_group = environment_group(
            device,
            layout,
            &EnvironmentBindings {
                parameters: &parameters,
                irradiance: &default_irradiance,
                specular: &default_specular_view,
                brdf: &brdf_view,
                sampler: &sampler,
            },
        );
        Self {
            default_group,
            parameters,
            sampler,
            brdf_view,
            _brdf: brdf,
            _default_irradiance: default_irradiance,
            _default_specular_view: default_specular_view,
            _default_specular: default_specular,
            format,
            capture: OnceLock::new(),
        }
    }

    /// The group bound when a frame has no environment.
    pub(crate) fn default_group(&self) -> &wgpu::BindGroup {
        &self.default_group
    }

    /// Upload this frame's parameters: intensity, rotation, and highest specular mip (zeros without an environment).
    pub(crate) fn write_parameters(&self, queue: &wgpu::Queue, lighting: Option<&ImageLighting<'_>>) {
        let parameters =
            lighting.map_or([0.0; 4], |lighting| [lighting.intensity, lighting.rotation, lighting.map.max_mip, 0.0]);
        queue.write_buffer(&self.parameters, 0, bytemuck::cast_slice(&parameters));
    }

    /// Prefilter `pixels` into specular and diffuse lighting bound with `layout`. Fails for malformed or oversized
    /// pixels.
    pub(crate) fn prepare(
        &self,
        gpu: &GpuContext,
        layout: &wgpu::BindGroupLayout,
        pixels: &EnvironmentPixels<'_>,
        options: &EnvironmentMapOptions,
    ) -> Result<EnvironmentMap, RenderError> {
        let device = gpu.device();
        let source = upload_source(gpu, pixels, options.encoding)?;
        let capture = self.capture.get_or_init(|| CapturePipelines::new(device, self.format));
        let specular = capture.prefiltered_specular(gpu, &source, options);

        // Diffuse irradiance integrates the source pixels themselves on the CPU.
        let srgb = options.encoding == EnvironmentEncoding::Srgb;
        let coefficients = match pixels.data {
            EnvironmentData::Unorm8(data) => irradiance_coefficients(pixels.width, pixels.height, |index| {
                std::array::from_fn(|channel| byte_radiance(data[(index * 4) + channel], srgb))
            }),
            EnvironmentData::Float(data) => irradiance_coefficients(pixels.width, pixels.height, |index| {
                std::array::from_fn(|channel| f64::from(data[(index * 4) + channel]))
            }),
        };
        let irradiance =
            uniform_buffer(device, "environment irradiance", std::mem::size_of::<IrradianceCoefficients>());
        gpu.queue().write_buffer(&irradiance, 0, bytemuck::cast_slice(&coefficients));

        let bind_group = environment_group(
            device,
            layout,
            &EnvironmentBindings {
                parameters: &self.parameters,
                irradiance: &irradiance,
                specular: &specular.view,
                brdf: &self.brdf_view,
                sampler: &self.sampler,
            },
        );
        Ok(EnvironmentMap {
            bind_group,
            _irradiance: irradiance,
            _specular_view: specular.view,
            _specular: specular.texture,
            max_mip: (specular.levels - 1) as f32,
        })
    }
}

/// What one image-lighting group binds.
struct EnvironmentBindings<'a> {
    parameters: &'a wgpu::Buffer,
    irradiance: &'a wgpu::Buffer,
    specular: &'a wgpu::TextureView,
    brdf: &'a wgpu::TextureView,
    sampler: &'a wgpu::Sampler,
}

fn environment_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    bindings: &EnvironmentBindings<'_>,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("image lighting"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: bindings.parameters.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: bindings.irradiance.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(bindings.specular),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(bindings.brdf),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(bindings.sampler),
            },
        ],
    })
}

/// Linear radiance of an eight-bit channel, decoding sRGB when `srgb`.
fn byte_radiance(value: u8, srgb: bool) -> f64 {
    let encoded = f64::from(value) / 255.0;
    if !srgb {
        return encoded;
    }
    if encoded <= 0.04045 { encoded / 12.92 } else { deterministic::powf((encoded + 0.055) / 1.055, 2.4) }
}

fn uniform_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
