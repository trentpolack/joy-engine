// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene-to-display pipeline: the HDR scene targets, bloom, and the display resolve.
//!
//! After the scene and its effects, bloom builds a mip pyramid from the exposed scene (prefilter, downsamples, then
//! upsamples that blend each wider level into the next finer one), and the resolve writes the presentation target:
//! exposure, bloom, vignette, the display transform (filmic, AgX, or none), HDR headroom, and the encoding
//! the target expects. See `post-bloom.wgsl` and `post-resolve.wgsl`.
//!
//! The scene targets (color, depth, and bloom) are the scene size, which the renderer's render scale can set below the
//! output size; the resolve upscales the scene into the output with a bicubic filter, so any UI drawn afterwards keeps
//! full output resolution. Targets are rebuilt only when sizes or the bloom pyramid's shape change; strengths,
//! thresholds, and tone curves are uniforms.
//!
//! Exposure is the authored multiplier times the value in a one-texel exposure texture, which adaptive exposure writes
//! and which holds one otherwise ([`PostProcessor::exposure_texture`]).

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::display_output::{
    DisplayOutput,
    OutputEncoding,
};
use crate::pipelines::{
    BLOOM_SOURCE,
    DEPTH_FORMAT,
    FullscreenTarget,
    draw_fullscreen,
    fullscreen_pipeline,
    resolve_source,
    sampler_entry,
    texture_entry,
    uniform_entry,
};
use crate::post_process::{
    AgxLook,
    BloomQuality,
    PostProcessSettings,
    ToneMapper,
};

/// Most bloom pyramid levels; the coarsest level of a long reach is then 1/256 of the scene.
const MAX_BLOOM_LEVELS: usize = 8;

/// Smallest bloom level extent in pixels.
const MIN_BLOOM_LEVEL_SIZE: u32 = 2;

/// Format of the exposure texture adaptive exposure writes: one texel, multiplier in red.
pub(crate) const EXPOSURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// One as an IEEE half float, for the exposure texture's reset.
const HALF_ONE: u16 = 0x3c00;

/// `Bloom` in `post-bloom.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct BloomUniforms {
    source: [f32; 4],
    pyramid: [f32; 4],
}

/// `Resolve` in `post-resolve.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ResolveUniforms {
    display: [f32; 4],
    filmic: [f32; 4],
    vignette: [f32; 4],
    output: [f32; 4],
    scene: [f32; 4],
    encoding: [f32; 4],
}

/// A texture with its default view, a view shaders can sample, and dimensions.
struct Target {
    // The views borrow the texture; drop them first.
    view: wgpu::TextureView,
    /// The default view, or for depth-stencil formats the depth aspect alone (shaders cannot bind both aspects).
    sample_view: wgpu::TextureView,
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

impl Target {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        // Color targets can be copied so scene effects write their results back into the scene color; depth formats
        // such as `Depth24Plus` cannot be copied at all.
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        if !format.is_depth_stencil_format() {
            usage |= wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sample_view = if format.has_stencil_aspect() {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some(label),
                aspect: wgpu::TextureAspect::DepthOnly,
                ..Default::default()
            })
        } else {
            view.clone()
        };
        Self {
            view,
            sample_view,
            texture,
            width: width.max(1),
            height: height.max(1),
        }
    }
}

/// The bloom pyramid's shape for a scene: how many levels, where the first one starts, and how much of the coarsest
/// level blends in (so the glow's reach changes smoothly between whole levels).
#[derive(Clone, Copy, Debug, PartialEq)]
struct BloomPyramid {
    shape: BloomShape,
    coarsest_weight: f32,
}

/// The pyramid's targets: level count and whether the first level is full resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BloomShape {
    levels: usize,
    full_resolution: bool,
}

impl BloomShape {
    /// Size of pyramid level `level` for a scene.
    fn level_size(&self, scene: (u32, u32), level: usize) -> (u32, u32) {
        let shift = level + usize::from(!self.full_resolution);
        (level_extent(scene.0, shift), level_extent(scene.1, shift))
    }
}

impl BloomPyramid {
    /// No bloom.
    const NONE: Self = Self {
        shape: BloomShape {
            levels: 0,
            full_resolution: false,
        },
        coarsest_weight: 0.0,
    };

    /// The pyramid that reaches `bloom_radius` of the shorter scene axis: each level doubles the footprint of a texel,
    /// so the coarsest level's footprint matches the reach.
    fn new(config: &PostProcessSettings, scene: (u32, u32)) -> Self {
        if config.bloom_strength <= 0.0 {
            return Self::NONE;
        }
        let full_resolution = config.bloom_quality == BloomQuality::Ultra;
        let reach = (config.bloom_radius * scene.0.min(scene.1) as f32).max(2.0);
        let exponent = reach.log2();
        let whole = exponent.ceil();
        let mut levels = (whole as usize + usize::from(full_resolution)).clamp(2, MAX_BLOOM_LEVELS);
        let mut coarsest_weight = (exponent - (whole - 1.0)).clamp(0.0, 1.0);

        // Stop before the coarsest level would shrink below a few pixels.
        let first_shift = if full_resolution { 0 } else { 1 };
        while levels > 1 && level_extent(scene.0.min(scene.1), first_shift + levels - 1) < MIN_BLOOM_LEVEL_SIZE {
            levels -= 1;
            coarsest_weight = 1.0;
        }
        Self {
            shape: BloomShape {
                levels,
                full_resolution,
            },
            coarsest_weight,
        }
    }
}

/// An extent halved `shift` times, at least one.
fn level_extent(extent: u32, shift: usize) -> u32 {
    (extent >> shift.min(31)).max(1)
}

/// Shape of the post graph; a change rebuilds targets and bind groups. Every field derives from the scene and output
/// sizes or the bloom pyramid's level count, so uniform-only changes (including radius within a level) keep the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GraphKey {
    /// Scene target size in pixels: the drawing size times the render scale.
    width: u32,
    height: u32,
    /// Presentation size in pixels.
    output_width: u32,
    output_height: u32,
    bloom: BloomShape,
}

impl GraphKey {
    fn new(config: &PostProcessSettings, scene: (u32, u32), output: (u32, u32)) -> Self {
        Self {
            width: scene.0,
            height: scene.1,
            output_width: output.0,
            output_height: output.1,
            bloom: BloomPyramid::new(config, scene).shape,
        }
    }

    /// Whether the resolve upscales a scene rendered below the output size.
    fn upscales(&self) -> bool {
        self.width < self.output_width || self.height < self.output_height
    }
}

/// What one bloom draw does; selects its pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BloomStage {
    /// Exposed, thresholded scene into the first level.
    Prefilter,
    /// One level into the next coarser one.
    Downsample,
    /// One level tent-filtered into the next finer one, blended by the step's weight.
    Upsample,
}

impl BloomStage {
    fn label(self) -> &'static str {
        match self {
            Self::Prefilter => "bloom prefilter",
            Self::Downsample => "bloom downsample",
            Self::Upsample => "bloom upsample",
        }
    }
}

/// One bound bloom draw.
struct BloomStep {
    stage: BloomStage,
    /// Index of the output level.
    output: usize,
    /// Texel size of the source, in UV.
    source_texel: [f32; 2],
    /// How much of the upsampled glow the output level takes.
    weight: f32,
    pass: Pass,
}

/// One full-screen pass bound to its source textures and uniform buffer.
struct Pass {
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
}

/// Size-dependent targets and the bound passes that read them.
struct Graph {
    key: GraphKey,
    scene: Target,
    depth: Target,
    bloom_levels: Vec<Target>,
    bloom_steps: Vec<BloomStep>,
    resolve: Pass,
}

/// Owns post-processing targets, pipelines, and the current display settings.
pub(crate) struct PostProcessor {
    // Pipelines and layouts outlive every graph.
    bloom_layout: wgpu::BindGroupLayout,
    resolve_layout: wgpu::BindGroupLayout,
    prefilter_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    upsample_pipeline: wgpu::RenderPipeline,
    resolve_pipeline: wgpu::RenderPipeline,
    linear_sampler: wgpu::Sampler,
    black: Target,

    // Exposure the resolve and bloom read.
    exposure: wgpu::Texture,
    exposure_view: wgpu::TextureView,

    // Configuration. The resolve writes the presentation target's signal; an sRGB view encodes it itself.
    scene_format: wgpu::TextureFormat,
    output_format: wgpu::TextureFormat,
    display: DisplayOutput,
    config: PostProcessSettings,

    // Size-dependent resources; `None` until the first prepare.
    graph: Option<Graph>,
}

impl PostProcessor {
    /// Compile post-processing pipelines for the scene format and presentation format.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene_format: wgpu::TextureFormat,
        output_format: wgpu::TextureFormat,
        config: PostProcessSettings,
    ) -> Self {
        let bloom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post bloom"),
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2), texture_entry(3)],
        });
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post resolve"),
            entries: &[uniform_entry(0), texture_entry(1), texture_entry(2), sampler_entry(3), texture_entry(4)],
        });

        let replace = wgpu::BlendState::REPLACE;
        let blend_by_constant = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Constant,
                dst_factor: wgpu::BlendFactor::OneMinusConstant,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let bloom = |label: &str, entry: &str, blend: wgpu::BlendState| {
            fullscreen_pipeline(device, label, BLOOM_SOURCE, entry, &[&bloom_layout], scene_format, blend)
        };
        let prefilter_pipeline = bloom("bloom-prefilter", "prefilterMain", replace);
        let downsample_pipeline = bloom("bloom-downsample", "downsampleMain", replace);
        let upsample_pipeline = bloom("bloom-upsample", "upsampleMain", blend_by_constant);
        let resolve_source = resolve_source();
        let resolve_pipeline = fullscreen_pipeline(
            device,
            "resolve",
            &resolve_source,
            "fragmentMain",
            &[&resolve_layout],
            output_format,
            replace,
        );

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black = Target::new(device, "post black", 1, 1, scene_format);

        let exposure = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("exposure"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: EXPOSURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let exposure_view = exposure.create_view(&wgpu::TextureViewDescriptor::default());

        let processor = Self {
            bloom_layout,
            resolve_layout,
            prefilter_pipeline,
            downsample_pipeline,
            upsample_pipeline,
            resolve_pipeline,
            linear_sampler,
            black,
            exposure,
            exposure_view,
            scene_format,
            output_format,
            display: DisplayOutput::STANDARD,
            config,
            graph: None,
        };
        processor.reset_exposure(queue);
        processor
    }

    /// The display output the resolve writes for.
    pub fn display_output(&self) -> DisplayOutput {
        self.display
    }

    /// Write for `display` from the next frame; encodings and headroom are uniforms.
    pub fn set_display_output(&mut self, display: DisplayOutput) {
        self.display = display;
    }

    /// Current post-processing settings.
    pub fn settings(&self) -> &PostProcessSettings {
        &self.config
    }

    /// Replace post-processing settings; targets are rebuilt on the next frame only if the bloom pyramid's shape
    /// changed.
    pub fn set_settings(&mut self, settings: PostProcessSettings) {
        self.config = settings;
    }

    /// HDR scene color view for the current frame. Requires [`prepare`](Self::prepare).
    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.graph.as_ref().expect("prepare runs before the scene pass").scene.view
    }

    /// HDR scene color texture for the current frame, for effects that copy their results into it. Requires
    /// [`prepare`](Self::prepare).
    pub fn scene_texture(&self) -> &wgpu::Texture {
        &self.graph.as_ref().expect("prepare runs before the scene pass").scene.texture
    }

    /// Scene depth and stencil attachment view for the current frame. Requires [`prepare`](Self::prepare).
    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self.graph.as_ref().expect("prepare runs before the scene pass").depth.view
    }

    /// Scene depth alone, for shaders that read it (`texture_depth_2d`). Requires [`prepare`](Self::prepare).
    pub fn depth_sample_view(&self) -> &wgpu::TextureView {
        &self.graph.as_ref().expect("prepare runs before the scene pass").depth.sample_view
    }

    /// The one-texel exposure multiplier ([`EXPOSURE_FORMAT`], red channel) that adaptive exposure copies into.
    pub fn exposure_texture(&self) -> &wgpu::Texture {
        &self.exposure
    }

    /// Return the exposure multiplier to one, as without adaptive exposure.
    pub fn reset_exposure(&self, queue: &wgpu::Queue) {
        let one = HALF_ONE.to_le_bytes();
        let texel = [one[0], one[1], one[0], one[1], one[0], one[1], one[0], one[1]];
        queue.write_texture(
            self.exposure.as_image_copy(),
            &texel,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: None,
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Ensure targets match the scene and output sizes (in pixels) and the bloom pyramid, then upload this frame's
    /// uniforms.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: (u32, u32), output: (u32, u32)) {
        let key = GraphKey::new(&self.config, scene, output);
        if self.graph.as_ref().map(|graph| graph.key) != Some(key) {
            self.graph = Some(self.build_graph(device, key));
        }
        self.write_uniforms(queue);
    }

    /// Record bloom and the resolve after the scene and its effects, ending in `output`.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let graph = self.graph.as_ref().expect("prepare runs before encode");

        for step in &graph.bloom_steps {
            let (pipeline, load) = match step.stage {
                BloomStage::Prefilter => (&self.prefilter_pipeline, wgpu::LoadOp::Clear(wgpu::Color::BLACK)),
                BloomStage::Downsample => (&self.downsample_pipeline, wgpu::LoadOp::Clear(wgpu::Color::BLACK)),
                BloomStage::Upsample => (&self.upsample_pipeline, wgpu::LoadOp::Load),
            };
            let weight = f64::from(step.weight);
            let target = FullscreenTarget {
                view: &graph.bloom_levels[step.output].view,
                load,
                blend_constant: Some(wgpu::Color {
                    r: weight,
                    g: weight,
                    b: weight,
                    a: weight,
                }),
            };
            draw_fullscreen(encoder, step.stage.label(), target, pipeline, &[&step.pass.bind_group]);
        }

        let target = FullscreenTarget::cleared(output);
        draw_fullscreen(encoder, "resolve", target, &self.resolve_pipeline, &[&graph.resolve.bind_group]);
    }

    /// Allocate targets for a graph shape and bind every pass.
    fn build_graph(&self, device: &wgpu::Device, key: GraphKey) -> Graph {
        let format = self.scene_format;
        let scene = Target::new(device, "scene color", key.width, key.height, format);
        let depth = Target::new(device, "scene depth", key.width, key.height, DEPTH_FORMAT);

        // The pyramid: the prefilter writes the first level, each downsample the next, then upsamples run back inward.
        let scene_size = (key.width, key.height);
        let bloom_levels: Vec<Target> = (0..key.bloom.levels)
            .map(|level| {
                let (width, height) = key.bloom.level_size(scene_size, level);
                Target::new(device, &format!("bloom level {level}"), width, height, format)
            })
            .collect();
        let texel = |target: &Target| [1.0 / target.width as f32, 1.0 / target.height as f32];
        let mut bloom_steps = Vec::new();
        for level in 0..bloom_levels.len() {
            let (stage, source, source_texel) = match level {
                0 => (BloomStage::Prefilter, &scene.view, [1.0 / key.width as f32, 1.0 / key.height as f32]),
                _ => (BloomStage::Downsample, &bloom_levels[level - 1].view, texel(&bloom_levels[level - 1])),
            };
            let pass = self.bloom_pass(device, source);
            bloom_steps.push(BloomStep {
                stage,
                output: level,
                source_texel,
                weight: 1.0,
                pass,
            });
        }
        for level in (0..bloom_levels.len().saturating_sub(1)).rev() {
            let coarser = &bloom_levels[level + 1];
            let pass = self.bloom_pass(device, &coarser.view);
            let source_texel = texel(coarser);
            bloom_steps.push(BloomStep {
                stage: BloomStage::Upsample,
                output: level,
                source_texel,
                weight: 0.0,
                pass,
            });
        }

        let glow = bloom_levels.first().map_or(&self.black.view, |level| &level.view);
        let resolve = self.resolve_pass(device, &scene.view, glow);
        Graph {
            key,
            scene,
            depth,
            bloom_levels,
            bloom_steps,
            resolve,
        }
    }

    /// Upload uniforms derived from the current settings and sizes.
    fn write_uniforms(&mut self, queue: &wgpu::Queue) {
        let graph = self.graph.as_mut().expect("graph exists after prepare");
        let config = &self.config;

        // Bloom steps: the upsample weights follow scatter, which is a uniform-only change.
        let quality = match config.bloom_quality {
            BloomQuality::Low => 0.0,
            BloomQuality::Medium => 1.0,
            BloomQuality::High | BloomQuality::Ultra => 2.0,
        };
        let levels = graph.bloom_levels.len();
        let coarsest_weight = BloomPyramid::new(config, (graph.key.width, graph.key.height)).coarsest_weight;
        for step in &mut graph.bloom_steps {
            if step.stage == BloomStage::Upsample {
                let fraction = if step.output + 2 == levels { coarsest_weight } else { 1.0 };
                step.weight = config.bloom_scatter * fraction;
            }
            let uniforms = BloomUniforms {
                source: [step.source_texel[0], step.source_texel[1], config.bloom_threshold, config.bloom_knee],
                pyramid: [config.exposure, quality, 0.0, 0.0],
            };
            queue.write_buffer(&step.pass.uniforms, 0, bytemuck::bytes_of(&uniforms));
        }

        let tone_mapper = match config.tone_mapper {
            ToneMapper::None => 0.0,
            ToneMapper::Filmic => 1.0,
            ToneMapper::Agx => 2.0,
        };
        let look = if config.agx_look == AgxLook::Punchy { 1.0 } else { 0.0 };
        let bloom_strength = if levels > 0 { config.bloom_strength } else { 0.0 };
        let (output_width, output_height) = (graph.key.output_width as f32, graph.key.output_height as f32);
        let (scene_width, scene_height) = (graph.key.width as f32, graph.key.height as f32);

        // Linear targets: sRGB-format views encode on write, and extended linear (scRGB) surfaces take linear light.
        let linear_target = self.output_format.is_srgb() || self.display.encoding == OutputEncoding::ExtendedLinear;

        // Untone-mapped light shows as it is; only the curves that end at SDR white extend into the headroom.
        let native_peak = matches!(config.tone_mapper, ToneMapper::None);
        let resolve = ResolveUniforms {
            display: [config.exposure, tone_mapper, look, bloom_strength],
            filmic: [
                config.filmic_contrast,
                config.filmic_linear_start,
                config.filmic_linear_length,
                config.filmic_toe,
            ],
            vignette: [config.vignette_strength, config.vignette_radius.min(0.999), output_width / output_height, 0.0],
            output: [
                self.display.effective_headroom(),
                if graph.key.upscales() { 1.0 } else { 0.0 },
                if linear_target { 1.0 } else { 0.0 },
                self.display.effective_sdr_white_level(),
            ],
            scene: [scene_width, scene_height, 1.0 / scene_width, 1.0 / scene_height],
            encoding: [dither_amplitude(self.output_format), if native_peak { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        queue.write_buffer(&graph.resolve.uniforms, 0, bytemuck::bytes_of(&resolve));
    }

    fn uniform_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn bloom_pass(&self, device: &wgpu::Device, source: &wgpu::TextureView) -> Pass {
        let uniforms = Self::uniform_buffer(device, "bloom uniforms", std::mem::size_of::<BloomUniforms>());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom"),
            layout: &self.bloom_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.exposure_view),
                },
            ],
        });
        Pass {
            bind_group,
            uniforms,
        }
    }

    fn resolve_pass(&self, device: &wgpu::Device, scene: &wgpu::TextureView, bloom: &wgpu::TextureView) -> Pass {
        let uniforms = Self::uniform_buffer(device, "resolve uniforms", std::mem::size_of::<ResolveUniforms>());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resolve"),
            layout: &self.resolve_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(scene),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(bloom),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.exposure_view),
                },
            ],
        });
        Pass {
            bind_group,
            uniforms,
        }
    }
}

/// One code value of an integer target, in encoded units; float targets need no dither.
fn dither_amplitude(format: wgpu::TextureFormat) -> f32 {
    match format.remove_srgb_suffix() {
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm => 1.0 / 255.0,
        wgpu::TextureFormat::Rgb10a2Unorm => 1.0 / 1023.0,
        _ => 0.0,
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
