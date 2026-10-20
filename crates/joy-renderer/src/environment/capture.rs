// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU passes that turn an equirectangular environment into a prefiltered specular cube, and the renderer's split-sum
//! BRDF table.
//!
//! Specular preparation captures the source into a radiance cube with a full mip chain (`environment-capture.wgsl`),
//! then prefilters each level of the specular cube from it with filtered importance sampling
//! (`environment-prefilter.wgsl`). Every pass draws one cube face; the work is recorded into one command buffer and
//! submitted, and the radiance cube is released once the queue finishes with it.

use crate::RenderError;
use crate::gpu::GpuContext;
use crate::pipelines::{
    sampler_entry,
    texture_entry,
};
use joy_core::math::deterministic;

use super::image_lighting::{
    EnvironmentData,
    EnvironmentEncoding,
    EnvironmentMapOptions,
    EnvironmentPixels,
};

const COMMON_SOURCE: &str = include_str!("../shaders/environment-common.wgsl");
const CAPTURE_SOURCE: &str = include_str!("../shaders/environment-capture.wgsl");
const PREFILTER_SOURCE: &str = include_str!("../shaders/environment-prefilter.wgsl");
const BRDF_SOURCE: &str = include_str!("../shaders/environment-brdf.wgsl");

/// Edge of the BRDF table in pixels (`BRDF_TABLE_SIZE` in environment-brdf.wgsl).
pub(crate) const BRDF_TABLE_SIZE: u32 = 128;

/// Edge of the specular cube's roughest level in pixels: a roughness-one lobe spans a hemisphere, which a few texels per
/// face resolve.
const MIN_SPECULAR_SIZE: u32 = 4;

/// Most equirectangular taps per axis for each radiance cube texel.
const MAX_CAPTURE_TAPS: u32 = 8;

/// Byte stride between per-draw uniforms; the portable minimum uniform offset alignment.
const PASS_STRIDE: u64 = 256;

/// Largest finite half-float value.
const HALF_MAX: f32 = 65504.0;

/// Cube faces per level.
const FACES: u32 = 6;

/// `EnvironmentPass` in environment-common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassUniforms {
    destination: [f32; 4],
    parameters: [f32; 4],
}

/// An uploaded equirectangular source.
pub(crate) struct SourceImage {
    view: wgpu::TextureView,
    width: u32,
    /// Whether the capture decodes sRGB.
    srgb: bool,
}

/// The prefiltered specular cube: one perceptual roughness per level.
pub(crate) struct SpecularCube {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// Levels from the mirror image (zero) to roughness one.
    pub levels: u32,
}

/// The capture and prefilter pipelines for one output format, compiled the first time an environment is prepared.
pub(crate) struct CapturePipelines {
    equirect_layout: wgpu::BindGroupLayout,
    cube_layout: wgpu::BindGroupLayout,
    equirect: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    prefilter: wgpu::RenderPipeline,
    equirect_sampler: wgpu::Sampler,
    cube_sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
}

/// One face draw: its pipeline, bind group, and target.
struct FaceDraw<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    group: &'a wgpu::BindGroup,
    target: wgpu::TextureView,
}

impl CapturePipelines {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Each layout holds only the bindings its entry points use.
        let pass_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<PassUniforms>() as u64),
            },
            count: None,
        };
        let cube_entry = wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float {
                    filterable: true,
                },
                view_dimension: wgpu::TextureViewDimension::Cube,
                multisampled: false,
            },
            count: None,
        };
        let equirect_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("environment equirect"),
            entries: &[pass_entry, texture_entry(1), sampler_entry(2)],
        });
        let cube_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("environment cube"),
            entries: &[pass_entry, sampler_entry(2), cube_entry],
        });

        let capture_source = format!("{COMMON_SOURCE}\n{CAPTURE_SOURCE}");
        let prefilter_source = format!("{COMMON_SOURCE}\n{PREFILTER_SOURCE}");
        let equirect =
            face_pipeline(device, "environment equirect", &capture_source, "equirectMain", &[&equirect_layout], format);
        let downsample =
            face_pipeline(device, "environment downsample", &capture_source, "downsampleMain", &[&cube_layout], format);
        let prefilter =
            face_pipeline(device, "environment prefilter", &prefilter_source, "prefilterMain", &[&cube_layout], format);

        let equirect_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment equirect"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let cube_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment cube"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            equirect_layout,
            cube_layout,
            equirect,
            downsample,
            prefilter,
            equirect_sampler,
            cube_sampler,
            format,
        }
    }

    /// Capture `source` and prefilter it into a specular cube of `options.size` texels per face edge.
    pub(crate) fn prefiltered_specular(
        &self,
        gpu: &GpuContext,
        source: &SourceImage,
        options: &EnvironmentMapOptions,
    ) -> SpecularCube {
        let device = gpu.device();
        let size = options.size.clamp(1, gpu.capabilities().max_texture_dimension);
        let sample_count = options.sample_count.clamp(1, 1024);
        let radiance_levels = u32::BITS - size.leading_zeros();
        let specular_levels = u32::BITS - (size / MIN_SPECULAR_SIZE).max(1).leading_zeros();
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
        let radiance = cube_texture(device, "environment radiance", size, radiance_levels, self.format, usage);
        let specular = cube_texture(device, "environment specular", size, specular_levels, self.format, usage);

        // Draws in order: the sharpest radiance level from the source, each smaller level from the one above, then
        // every specular level from the finished radiance cube.
        let taps = source.width.div_ceil(4 * size).clamp(1, MAX_CAPTURE_TAPS);
        let mut draws: Vec<(DrawKind, u32, u32, PassUniforms)> = Vec::new();
        for face in 0..FACES {
            let uniforms = PassUniforms {
                destination: [face as f32, size as f32, size as f32, 1.0],
                parameters: [0.0, radiance_levels as f32, if source.srgb { 1.0 } else { 0.0 }, taps as f32],
            };
            draws.push((DrawKind::Equirect, 0, face, uniforms));
        }
        for level in 1..radiance_levels {
            for face in 0..FACES {
                let uniforms = PassUniforms {
                    destination: [face as f32, (size >> level).max(1) as f32, size as f32, 1.0],
                    parameters: [0.0, radiance_levels as f32, 0.0, 1.0],
                };
                draws.push((DrawKind::Downsample(level - 1), level, face, uniforms));
            }
        }
        for level in 0..specular_levels {
            let roughness = level_roughness(level, specular_levels);
            for face in 0..FACES {
                let uniforms = PassUniforms {
                    destination: [face as f32, (size >> level).max(1) as f32, size as f32, sample_count as f32],
                    parameters: [roughness, radiance_levels as f32, 0.0, 1.0],
                };
                draws.push((DrawKind::Prefilter, level, face, uniforms));
            }
        }

        // Per-draw uniforms at aligned offsets in one buffer.
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("environment passes"),
            size: draws.len() as u64 * PASS_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut bytes = vec![0_u8; draws.len() * PASS_STRIDE as usize];
        for (chunk, (_, _, _, uniforms)) in bytes.as_chunks_mut::<{ PASS_STRIDE as usize }>().0.iter_mut().zip(&draws) {
            chunk[..std::mem::size_of::<PassUniforms>()].copy_from_slice(bytemuck::bytes_of(uniforms));
        }
        gpu.queue().write_buffer(&uniform_buffer, 0, &bytes);

        // Bind groups: the equirectangular source, each radiance level read by the next, and the whole radiance cube.
        let equirect_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment equirect"),
            layout: &self.equirect_layout,
            entries: &[
                self.pass_binding(&uniform_buffer),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&source.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.equirect_sampler),
                },
            ],
        });
        let cube_group = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("environment cube"),
                layout: &self.cube_layout,
                entries: &[
                    self.pass_binding(&uniform_buffer),
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.cube_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                ],
            })
        };
        let level_groups: Vec<wgpu::BindGroup> =
            (0..radiance_levels.saturating_sub(1)).map(|level| cube_group(&cube_view(&radiance, level, 1))).collect();
        let radiance_group = cube_group(&cube_view(&radiance, 0, radiance_levels));

        let face_draws = draws.iter().map(|(kind, level, face, _)| {
            let (pipeline, group, target) = match kind {
                DrawKind::Equirect => (&self.equirect, &equirect_group, &radiance),
                DrawKind::Downsample(source_level) => {
                    (&self.downsample, &level_groups[*source_level as usize], &radiance)
                }
                DrawKind::Prefilter => (&self.prefilter, &radiance_group, &specular),
            };
            FaceDraw {
                pipeline,
                group,
                target: face_view(target, *level, *face),
            }
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("environment capture"),
        });
        for (index, draw) in face_draws.enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("environment face"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &draw.target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(draw.pipeline);
            pass.set_bind_group(0, draw.group, &[(index as u64 * PASS_STRIDE) as u32]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue().submit([encoder.finish()]);

        let view = cube_view(&specular, 0, specular_levels);
        SpecularCube {
            texture: specular,
            view,
            levels: specular_levels,
        }
    }

    fn pass_binding<'a>(&self, buffer: &'a wgpu::Buffer) -> wgpu::BindGroupEntry<'a> {
        wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: wgpu::BufferSize::new(std::mem::size_of::<PassUniforms>() as u64),
            }),
        }
    }
}

/// What a face draw reads.
#[derive(Clone, Copy)]
enum DrawKind {
    /// The equirectangular source.
    Equirect,
    /// The radiance cube's level above.
    Downsample(u32),
    /// The whole radiance cube.
    Prefilter,
}

/// Integrate the split-sum BRDF table into a new texture of `format`; submits its own work.
pub(crate) fn brdf_table(gpu: &GpuContext, format: wgpu::TextureFormat) -> wgpu::Texture {
    let device = gpu.device();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("environment brdf"),
        size: wgpu::Extent3d {
            width: BRDF_TABLE_SIZE,
            height: BRDF_TABLE_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let source = format!("{COMMON_SOURCE}\n{BRDF_SOURCE}");
    let pipeline = face_pipeline(device, "environment brdf", &source, "brdfMain", &[], format);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("environment brdf"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("environment brdf"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    gpu.queue().submit([encoder.finish()]);
    texture
}

/// Validate and upload equirectangular pixels; byte channels decode sRGB in the capture when `encoding` says so.
pub(crate) fn upload_source(
    gpu: &GpuContext,
    pixels: &EnvironmentPixels<'_>,
    encoding: EnvironmentEncoding,
) -> Result<SourceImage, RenderError> {
    let invalid = |message: &str| RenderError::InvalidTexture(message.to_owned());
    let max_dimension = gpu.capabilities().max_texture_dimension;
    let (width, height) = (pixels.width, pixels.height);
    let channel_count = (width as usize) * (height as usize) * 4;
    let length = match pixels.data {
        EnvironmentData::Unorm8(data) => data.len(),
        EnvironmentData::Float(data) => data.len(),
    };
    if width == 0 || height == 0 || width > max_dimension || height > max_dimension || length != channel_count {
        return Err(invalid("Environment pixels need supported dimensions and four channels per pixel."));
    }
    let (format, bytes) = match pixels.data {
        EnvironmentData::Unorm8(data) => (wgpu::TextureFormat::Rgba8Unorm, data.to_vec()),
        EnvironmentData::Float(data) => {
            if data.iter().any(|value| !value.is_finite() || *value < 0.0 || *value > HALF_MAX) {
                return Err(invalid("HDR environment pixels must be finite, nonnegative half-float values."));
            }
            let halves: Vec<u16> = data.iter().map(|value| encode_positive_half(*value)).collect();
            (wgpu::TextureFormat::Rgba16Float, bytemuck::cast_slice(&halves).to_vec())
        }
    };
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("environment source"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes_per_pixel = format.block_copy_size(None).unwrap_or(4);
    gpu.queue().write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_pixel),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let srgb = encoding == EnvironmentEncoding::Srgb && format == wgpu::TextureFormat::Rgba8Unorm;
    Ok(SourceImage {
        view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        width,
        srgb,
    })
}

/// The perceptual roughness specular level `level` of `levels` holds: the inverse of the lookup's `r(2 - r)` mapping
/// (`environmentSpecular` in environment-lighting.wgsl).
pub(crate) fn level_roughness(level: u32, levels: u32) -> f32 {
    if levels <= 1 {
        return 0.0;
    }
    let fraction = level as f32 / (levels - 1) as f32;
    1.0 - (1.0 - fraction).max(0.0).sqrt()
}

/// Convert a validated nonnegative value to an IEEE half float, including subnormals and mantissa carry.
pub(crate) fn encode_positive_half(value: f32) -> u16 {
    let value = f64::from(value);
    if value < deterministic::powi(2_f64, -14) {
        return (value * deterministic::powi(2_f64, 24)).round() as u16;
    }
    let exponent = deterministic::log2(value).floor();
    ((exponent + 15.0) * 1024.0 + ((value / deterministic::powf(2_f64, exponent) - 1.0) * 1024.0).round()) as u16
}

/// A six-layer, cube-compatible texture of `size` texels per face edge.
pub(crate) fn cube_texture(
    device: &wgpu::Device,
    label: &str,
    size: u32,
    levels: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: FACES,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// A cube view of `count` levels starting at `level`.
pub(crate) fn cube_view(texture: &wgpu::Texture, level: u32, count: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("environment cube"),
        dimension: Some(wgpu::TextureViewDimension::Cube),
        base_mip_level: level,
        mip_level_count: Some(count),
        ..Default::default()
    })
}

/// A 2D view of one face of one level, for rendering into.
fn face_view(texture: &wgpu::Texture, level: u32, face: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("environment face"),
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_mip_level: level,
        mip_level_count: Some(1),
        base_array_layer: face,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

/// A pipeline that draws one full-target triangle with `entry` into `format`.
fn face_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    entry: &str,
    layouts: &[&wgpu::BindGroupLayout],
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(|layout| Some(*layout)).collect();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
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
            entry_point: Some(entry),
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
}

#[cfg(test)]
#[path = "capture.test.rs"]
mod tests;
