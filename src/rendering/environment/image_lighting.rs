// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Image-based lighting for PBR surfaces, ported from luma.gl's `PBREnvironmentGenerator` (which the TypeScript
//! `PbrEnvironment` uses): an equirectangular environment is integrated into a cosine-weighted diffuse irradiance cube,
//! a GGX-prefiltered specular cube with one roughness per mip, and a split-sum BRDF table.
//!
//! Preparation submits its own GPU work; call it during setup or when the environment changes, never per frame. The
//! intensity and horizontal rotation are per-frame inputs ([`ImageLighting`]) and never regenerate textures.

use crate::rendering::RenderError;
use crate::rendering::gpu::GpuContext;

const FILTER_SOURCE: &str = include_str!("../shaders/environment-filter.wgsl");
/// Byte stride between per-draw filter uniforms; the portable minimum uniform offset alignment.
const FILTER_STRIDE: u64 = 256;
/// Largest finite half-float value.
const HALF_MAX: f32 = 65504.0;

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

/// Integration quality, matching the luma.gl defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentMapOptions {
    /// Source color encoding.
    pub encoding: EnvironmentEncoding,
    /// Base size of the prefiltered specular cube, in pixels.
    pub size: u32,
    /// Size of the diffuse irradiance cube, in pixels.
    pub irradiance_size: u32,
    /// Size of the BRDF table, in pixels.
    pub brdf_lut_size: u32,
    /// Importance samples per destination texel, at most 1024.
    pub sample_count: u32,
}

impl Default for EnvironmentMapOptions {
    fn default() -> Self {
        Self {
            encoding: EnvironmentEncoding::Linear,
            size: 64,
            irradiance_size: 16,
            brdf_lut_size: 128,
            sample_count: 64,
        }
    }
}

/// Prepared lighting textures. Owned by the caller and borrowed per frame through [`ImageLighting`]; drop it before the
/// device.
pub struct EnvironmentMap {
    // The bind group references the views; drop it first.
    pub(crate) bind_group: wgpu::BindGroup,
    _views: [wgpu::TextureView; 3],
    _textures: [wgpu::Texture; 3],
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

/// The image-lighting group: per-frame uniforms, both cubes, the BRDF table, and one filtering sampler.
pub(crate) fn image_lighting_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("image lighting"),
        entries: &[
            crate::rendering::materials::uniform_entry(0),
            texture(1, wgpu::TextureViewDimension::Cube),
            texture(2, wgpu::TextureViewDimension::Cube),
            texture(3, wgpu::TextureViewDimension::D2),
            crate::rendering::materials::sampler_entry(4),
        ],
    })
}

/// Output format: half float where it is renderable and filterable, as luma.gl chooses.
pub(crate) fn environment_format(scene_format: wgpu::TextureFormat) -> wgpu::TextureFormat {
    if scene_format == wgpu::TextureFormat::Rgba16Float {
        wgpu::TextureFormat::Rgba16Float
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    }
}

/// Build the image-lighting bind group for a set of views.
pub(crate) fn image_lighting_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    views: &[wgpu::TextureView; 3],
) -> wgpu::BindGroup {
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("image lighting"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("image lighting"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[2]) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    })
}

/// Create the three lighting textures: diffuse cube, specular cube, and BRDF table.
pub(crate) fn lighting_textures(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: u32,
    irradiance_size: u32,
    lut_size: u32,
    specular_mips: u32,
) -> [wgpu::Texture; 3] {
    let texture = |label, size: u32, layers, mips| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: layers },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    };
    [
        texture("environment diffuse", irradiance_size, 6, 1),
        texture("environment specular", size, 6, specular_mips),
        texture("environment brdf", lut_size, 1, 1),
    ]
}

/// Sampling views of the lighting textures.
pub(crate) fn lighting_views(textures: &[wgpu::Texture; 3]) -> [wgpu::TextureView; 3] {
    let cube = |texture: &wgpu::Texture| {
        texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        })
    };
    [cube(&textures[0]), cube(&textures[1]), textures[2].create_view(&wgpu::TextureViewDescriptor::default())]
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FilterUniforms {
    face: u32,
    mode: u32,
    sample_count: u32,
    source_encoding: u32,
    roughness: f32,
    size: f32,
    padding: [f32; 2],
}

/// Integrate `pixels` into lighting textures and bind them for the PBR pass.
pub(crate) fn prepare_environment_map(
    gpu: &GpuContext,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    pixels: &EnvironmentPixels<'_>,
    options: &EnvironmentMapOptions,
) -> Result<EnvironmentMap, RenderError> {
    let (device, queue) = (gpu.device(), gpu.queue());
    let (scene_format, max_texture_dimension) =
        (gpu.capabilities().scene_format, gpu.capabilities().max_texture_dimension);
    let invalid = |message: &str| RenderError::InvalidTexture(message.to_owned());
    let (width, height) = (pixels.width, pixels.height);
    let channel_count = (width as usize) * (height as usize) * 4;
    let length = match pixels.data {
        EnvironmentData::Unorm8(data) => data.len(),
        EnvironmentData::Float(data) => data.len(),
    };
    if width == 0
        || height == 0
        || width > max_texture_dimension
        || height > max_texture_dimension
        || length != channel_count
    {
        return Err(invalid("Environment pixels need supported dimensions and four channels per pixel."));
    }
    let (source_format, bytes) = match pixels.data {
        EnvironmentData::Unorm8(data) => (wgpu::TextureFormat::Rgba8Unorm, data.to_vec()),
        EnvironmentData::Float(data) => {
            if data.iter().any(|value| !value.is_finite() || *value < 0.0 || *value > HALF_MAX) {
                return Err(invalid("HDR environment pixels must be finite, nonnegative half-float values."));
            }
            let halves: Vec<u16> = data.iter().map(|value| encode_positive_half(*value)).collect();
            (wgpu::TextureFormat::Rgba16Float, bytemuck::cast_slice(&halves).to_vec())
        }
    };
    let source = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("environment source"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: source_format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes_per_pixel = source_format.block_copy_size(None).unwrap_or(4);
    queue.write_texture(
        source.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * bytes_per_pixel), rows_per_image: None },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    let size = options.size.clamp(1, max_texture_dimension);
    let irradiance_size = options.irradiance_size.clamp(1, max_texture_dimension);
    let lut_size = options.brdf_lut_size.clamp(1, max_texture_dimension);
    let sample_count = options.sample_count.clamp(1, 1024);
    let mips = u32::BITS - size.leading_zeros();
    let format = environment_format(scene_format);
    let textures = lighting_textures(device, format, size, irradiance_size, lut_size, mips);

    // One draw per specular face and mip, per diffuse face, and one for the BRDF table.
    let mut draws = Vec::new();
    for mip in 0..mips {
        let roughness = if mips > 1 { mip as f32 / (mips - 1) as f32 } else { 0.0 };
        for face in 0..6 {
            draws.push((1_usize, face, mip, (size >> mip).max(1), 0_u32, roughness));
        }
    }
    for face in 0..6 {
        draws.push((0, face, 0, irradiance_size, 1, 1.0));
    }
    draws.push((2, 0, 0, lut_size, 2, 0.0));

    let encoding =
        u32::from(options.encoding == EnvironmentEncoding::Srgb && source_format == wgpu::TextureFormat::Rgba8Unorm);
    let filter_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("environment filter"),
        size: draws.len() as u64 * FILTER_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut filter_bytes = vec![0_u8; draws.len() * FILTER_STRIDE as usize];
    for (chunk, (_, face, _, target, mode, roughness)) in
        filter_bytes.as_chunks_mut::<{ FILTER_STRIDE as usize }>().0.iter_mut().zip(&draws)
    {
        let uniforms = FilterUniforms {
            face: *face,
            mode: *mode,
            sample_count,
            source_encoding: encoding,
            roughness: *roughness,
            size: *target as f32,
            padding: [0.0; 2],
        };
        chunk[..std::mem::size_of::<FilterUniforms>()].copy_from_slice(bytemuck::bytes_of(&uniforms));
    }
    queue.write_buffer(&filter_buffer, 0, &filter_bytes);

    let filter_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("environment filter"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<FilterUniforms>() as u64),
                },
                count: None,
            },
            crate::rendering::materials::texture_entry(1),
            crate::rendering::materials::sampler_entry(2),
        ],
    });
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
    let source_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("environment source"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let filter_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("environment filter"),
        layout: &filter_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &filter_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<FilterUniforms>() as u64),
                }),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&source_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&source_sampler) },
        ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("environment filter"),
        source: wgpu::ShaderSource::Wgsl(FILTER_SOURCE.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("environment filter"),
        bind_group_layouts: &[Some(&filter_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("environment filter"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("environment integration") });
    for (draw, (texture, face, mip, _, _, _)) in draws.iter().enumerate() {
        let view = textures[*texture].create_view(&wgpu::TextureViewDescriptor {
            label: Some("environment face"),
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_mip_level: *mip,
            mip_level_count: Some(1),
            base_array_layer: *face,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("environment integration"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &filter_group, &[(draw as u64 * FILTER_STRIDE) as u32]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);

    let views = lighting_views(&textures);
    let bind_group = image_lighting_group(device, layout, uniforms, &views);
    Ok(EnvironmentMap { bind_group, _views: views, _textures: textures, max_mip: (mips - 1) as f32 })
}

/// Convert a validated nonnegative value to an IEEE half float, including subnormals and mantissa carry, like the
/// TypeScript `encodePositiveHalf`.
fn encode_positive_half(value: f32) -> u16 {
    let value = f64::from(value);
    if value < 2_f64.powi(-14) {
        return (value * 2_f64.powi(24)).round() as u16;
    }
    let exponent = value.log2().floor();
    ((exponent + 15.0) * 1024.0 + ((value / 2_f64.powf(exponent) - 1.0) * 1024.0).round()) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_match_ieee_encoding() {
        assert_eq!(encode_positive_half(0.0), 0);
        assert_eq!(encode_positive_half(1.0), 0x3c00);
        assert_eq!(encode_positive_half(0.5), 0x3800);
        assert_eq!(encode_positive_half(65504.0), 0x7bff);
        assert_eq!(encode_positive_half(2_f32.powi(-24)), 1);
        // Rounding up the mantissa carries into the exponent.
        assert_eq!(encode_positive_half(1.9999), 0x4000);
    }
}
