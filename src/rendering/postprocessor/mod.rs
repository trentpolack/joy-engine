// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene-to-display pipeline: HDR scene targets, bloom, blurs, the display resolve, and the final presentation pass.
//!
//! The pass order matches the TypeScript engine: bloom, optional full-image blur and vignette edge blur, exposure and
//! tone mapping, grade, vignette, antialiasing, then film grain. Bloom is the fragment path of luma.gl's
//! `createBloomShaderPassPipeline` with the options the TypeScript engine passes (MIT License, Copyright (c) vis.gl
//! contributors; see `post-bloom.wgsl` and `scene_effects/LICENSE-luma.gl`): a two-to-five level pyramid of
//! thresholded tent extraction and separable Gaussians, bicubic reconstruction blended by scatter, optional horizontal
//! starburst streaks, and an additive composite. Targets are rebuilt only when the drawing size or the effect graph
//! (bloom levels, streaks, blur passes) changes, which the TypeScript `effectPipelineKey` also keys on; radii,
//! thresholds, and intensities are plain uniforms.

use crate::core::post_processing::{AgxLook, FilmGrainNoise, PostProcessingConfig, ToneMapper};
use bytemuck::{Pod, Zeroable};

use crate::rendering::materials::{
    BLOOM_SOURCE, BLUR_SOURCE, DEPTH_FORMAT, DISPLAY_FORMAT, FINAL_SOURCE, RESOLVE_SOURCE, fullscreen_pipeline,
    sampler_entry, texture_entry, uniform_entry,
};

/// Maps the authored bloom radius to the TypeScript renderer's kernel scale: 0.025 at 720 pixels is eight texels.
const BLOOM_RADIUS_SCALE: f32 = 8.0 / (720.0 * 0.025);

/// Upper bound on the bloom kernel in texels, matching the TypeScript renderer.
const MAX_BLOOM_RADIUS: f32 = 24.0;

/// luma.gl's `MAX_BLOOM_BLUR_RADIUS`: the anamorphic stretch keeps each axis within it.
const MAX_BLOOM_BLUR_RADIUS: f32 = 24.0;

/// Pyramid level sizes relative to the drawing buffer, widest first (luma.gl's `BLOOM_PYRAMID_LEVELS`).
const BLOOM_LEVEL_SCALES: [f64; 5] = [0.5, 0.25, 0.125, 0.0625, 0.03125];

/// Starburst ray length in half-resolution texels per bloom radius per shortest-axis pixel, as the TypeScript renderer
/// requests it.
const STARBURST_LENGTH_SCALE: f32 = 4.0;

/// luma.gl's longest starburst ray in half-resolution texels.
const MAX_STARBURST_LENGTH: f32 = 256.0;

/// Upper bound on the full-image Gaussian radius in texels.
const MAX_BLUR_RADIUS: f32 = 32.0;

/// Edge-blur radius in half-resolution texels, matching the TypeScript vignette blur.
const EDGE_BLUR_RADIUS: f32 = 6.0;

/// One bloom draw's uniforms (`Bloom` in `post-bloom.wgsl`); each stage reads the fields it needs.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct BloomUniforms {
    texture_scale: [f32; 2],
    delta: [f32; 2],
    threshold: f32,
    soft_knee: f32,
    exposure: f32,
    radius: f32,
    scatter: f32,
    intensity: f32,
    starburst_intensity: f32,
    starburst_length: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BlurUniforms {
    direction: [f32; 2],
    radius: f32,
    padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ResolveUniforms {
    display: [f32; 4],
    grade: [f32; 4],
    vignette: [f32; 4],
    viewport: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PresentationUniforms {
    frame: [f32; 4],
    grain: [f32; 4],
}

/// A texture with its default view and dimensions.
struct Target {
    // The view borrows the texture; drop it first.
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    width: u32,
    height: u32,
}

impl Target {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view, _texture: texture, width: width.max(1), height: height.max(1) }
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Shape of the effect graph; a change rebuilds targets and bind groups. Every field derives from the drawing size or
/// a value the TypeScript `effectPipelineKey` includes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GraphKey {
    width: u32,
    height: u32,
    bloom_levels: usize,
    lens_flare: bool,
    full_blur: bool,
    edge_blur: bool,
}

impl GraphKey {
    fn new(config: &PostProcessingConfig, width: u32, height: u32) -> Self {
        let bloom = config.bloom_strength > 0.0;
        Self {
            width,
            height,
            bloom_levels: if bloom { config.bloom_quality.levels() } else { 0 },
            lens_flare: bloom && config.bloom_lens_flare > 0.0,
            full_blur: config.blur_radius > 0.0,
            edge_blur: config.vignette_blur > 0.0,
        }
    }

    /// Whether a blur reads the bloomed scene, so the bloom composite must run before it rather than in the resolve.
    fn composes_bloom_early(&self) -> bool {
        self.bloom_levels > 0 && (self.full_blur || self.edge_blur)
    }
}

/// What one bloom draw computes; selects its pipeline and uniforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BloomStage {
    /// Thresholded tent from the scene into the first level.
    Extract,
    /// Tent from the previous level's extraction.
    Downsample,
    /// Horizontal Gaussian.
    BlurHorizontal,
    /// Vertical Gaussian.
    BlurVertical,
    /// Bicubic wider glow blended with this level's blur.
    Upsample,
    /// Starburst streaks from the reconstructed glow.
    Lens,
    /// Scene plus glow and streaks, ahead of a blur.
    Composite,
}

impl BloomStage {
    fn label(self) -> &'static str {
        match self {
            Self::Extract => "bloom extract",
            Self::Downsample => "bloom downsample",
            Self::BlurHorizontal => "bloom blur horizontal",
            Self::BlurVertical => "bloom blur vertical",
            Self::Upsample => "bloom upsample",
            Self::Lens => "bloom lens",
            Self::Composite => "bloom composite",
        }
    }
}

/// One bound bloom draw.
struct BloomStep {
    stage: BloomStage,
    /// Index of the output in `Graph::bloom_targets`.
    output: usize,
    /// luma.gl's aspect correction between this draw's source and output.
    texture_scale: [f32; 2],
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
    display: Target,
    bloom_targets: Vec<Target>,
    bloom_steps: Vec<BloomStep>,
    blur_targets: Option<(Target, Target)>,
    edge_targets: Option<(Target, Target)>,
    blur_passes: Vec<Pass>,
    edge_passes: Vec<Pass>,
    resolve: Pass,
    presentation: Pass,
}

/// Owns post-processing targets, pipelines, and the current display settings.
pub(crate) struct PostProcessor {
    // Pipelines and layouts outlive every graph.
    bloom_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    resolve_layout: wgpu::BindGroupLayout,
    presentation_layout: wgpu::BindGroupLayout,
    extract_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    bloom_blur_pipeline: wgpu::RenderPipeline,
    upsample_pipeline: wgpu::RenderPipeline,
    lens_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    resolve_pipeline: wgpu::RenderPipeline,
    presentation_pipeline: wgpu::RenderPipeline,
    linear_sampler: wgpu::Sampler,
    black: Target,

    // Configuration.
    scene_format: wgpu::TextureFormat,
    config: PostProcessingConfig,

    // Size-dependent resources; `None` until the first resize.
    graph: Option<Graph>,
}

impl PostProcessor {
    /// Compile post-processing pipelines for the scene format and presentation format.
    pub fn new(
        device: &wgpu::Device,
        scene_format: wgpu::TextureFormat,
        output_format: wgpu::TextureFormat,
        config: PostProcessingConfig,
    ) -> Self {
        let bloom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post bloom"),
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2), texture_entry(3), texture_entry(4)],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post blur"),
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2)],
        });
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post resolve"),
            entries: &[
                uniform_entry(0),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                sampler_entry(4),
                texture_entry(5),
            ],
        });
        let presentation_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post presentation"),
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2)],
        });

        let bloom = |label: &str, entry: &str| {
            fullscreen_pipeline(device, label, BLOOM_SOURCE, entry, &bloom_layout, scene_format)
        };
        let extract_pipeline = bloom("bloom-extract", "extractMain");
        let downsample_pipeline = bloom("bloom-downsample", "downsampleMain");
        let bloom_blur_pipeline = bloom("bloom-blur", "blurMain");
        let upsample_pipeline = bloom("bloom-upsample", "upsampleMain");
        let lens_pipeline = bloom("bloom-lens", "lensMain");
        let composite_pipeline = bloom("bloom-composite", "compositeMain");
        let blur_pipeline =
            fullscreen_pipeline(device, "blur", BLUR_SOURCE, "fragmentMain", &blur_layout, scene_format);
        let resolve_pipeline =
            fullscreen_pipeline(device, "resolve", RESOLVE_SOURCE, "fragmentMain", &resolve_layout, DISPLAY_FORMAT);
        let presentation_pipeline = fullscreen_pipeline(
            device,
            "presentation",
            FINAL_SOURCE,
            "fragmentMain",
            &presentation_layout,
            output_format,
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

        Self {
            bloom_layout,
            blur_layout,
            resolve_layout,
            presentation_layout,
            extract_pipeline,
            downsample_pipeline,
            bloom_blur_pipeline,
            upsample_pipeline,
            lens_pipeline,
            composite_pipeline,
            blur_pipeline,
            resolve_pipeline,
            presentation_pipeline,
            linear_sampler,
            black,
            scene_format,
            config,
            graph: None,
        }
    }

    /// Current display settings.
    pub fn config(&self) -> &PostProcessingConfig {
        &self.config
    }

    /// Replace display settings; the effect graph is rebuilt on the next frame only if its shape changed.
    pub fn set_config(&mut self, config: PostProcessingConfig) {
        self.config = config;
    }

    /// HDR scene color view for the current frame. Requires [`prepare`](Self::prepare).
    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.graph.as_ref().expect("prepare runs before the scene pass").scene.view
    }

    /// Scene depth view for the current frame. Requires [`prepare`](Self::prepare).
    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self.graph.as_ref().expect("prepare runs before the scene pass").depth.view
    }

    /// Ensure targets match the drawing size and effect graph, then upload this frame's uniforms.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, time_seconds: f32) {
        let key = GraphKey::new(&self.config, width, height);
        if self.graph.as_ref().map(|graph| graph.key) != Some(key) {
            self.graph = Some(self.build_graph(device, key));
        }
        self.write_uniforms(queue, time_seconds);
    }

    /// Record every post-processing pass after the scene pass, ending in `output`.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let graph = self.graph.as_ref().expect("prepare runs before encode");

        // Bloom: per level extraction or downsample and two Gaussians, reconstruction inward, streaks, early composite.
        for step in &graph.bloom_steps {
            let pipeline = match step.stage {
                BloomStage::Extract => &self.extract_pipeline,
                BloomStage::Downsample => &self.downsample_pipeline,
                BloomStage::BlurHorizontal | BloomStage::BlurVertical => &self.bloom_blur_pipeline,
                BloomStage::Upsample => &self.upsample_pipeline,
                BloomStage::Lens => &self.lens_pipeline,
                BloomStage::Composite => &self.composite_pipeline,
            };
            fullscreen(encoder, step.stage.label(), &graph.bloom_targets[step.output].view, pipeline, &step.pass);
        }

        // Full-image blur: horizontal into the first target, vertical into the second.
        if let Some((horizontal, vertical)) = &graph.blur_targets {
            fullscreen(encoder, "blur horizontal", &horizontal.view, &self.blur_pipeline, &graph.blur_passes[0]);
            fullscreen(encoder, "blur vertical", &vertical.view, &self.blur_pipeline, &graph.blur_passes[1]);
        }

        // Vignette edge blur at half resolution.
        if let Some((horizontal, vertical)) = &graph.edge_targets {
            fullscreen(encoder, "edge blur horizontal", &horizontal.view, &self.blur_pipeline, &graph.edge_passes[0]);
            fullscreen(encoder, "edge blur vertical", &vertical.view, &self.blur_pipeline, &graph.edge_passes[1]);
        }

        fullscreen(encoder, "resolve", &graph.display.view, &self.resolve_pipeline, &graph.resolve);
        fullscreen(encoder, "presentation", output, &self.presentation_pipeline, &graph.presentation);
    }

    /// Allocate targets for a graph shape and bind every pass.
    fn build_graph(&self, device: &wgpu::Device, key: GraphKey) -> Graph {
        let format = self.scene_format;
        let scene = Target::new(device, "scene color", key.width, key.height, format);
        let depth = Target::new(device, "scene depth", key.width, key.height, DEPTH_FORMAT);
        let display = Target::new(device, "display", key.width, key.height, DISPLAY_FORMAT);

        // Bloom targets: each level's extraction, Gaussian scratch, and blur, then the streaks and the early composite.
        // Reconstruction writes each level's extraction again once it is no longer read, as luma.gl aliases them.
        let mut bloom_targets = Vec::new();
        let mut levels: Vec<[usize; 3]> = Vec::new();
        for (level, scale) in BLOOM_LEVEL_SCALES.iter().take(key.bloom_levels).enumerate() {
            let (width, height) = (level_size(key.width, *scale), level_size(key.height, *scale));
            let first = bloom_targets.len();
            for role in ["extract", "scratch", "blur"] {
                bloom_targets.push(Target::new(device, &format!("bloom {role} {level}"), width, height, format));
            }
            levels.push([first, first + 1, first + 2]);
        }
        let lens = (key.lens_flare && !levels.is_empty()).then(|| {
            let half = &bloom_targets[levels[0][0]];
            let (width, height) = half.size();
            bloom_targets.push(Target::new(device, "bloom lens", width, height, format));
            bloom_targets.len() - 1
        });
        let composite = key.composes_bloom_early().then(|| {
            bloom_targets.push(Target::new(device, "bloom composite", key.width, key.height, format));
            bloom_targets.len() - 1
        });

        let mut bloom_steps = Vec::new();
        let black = &self.black.view;
        let mut step =
            |stage, source: &Target, glow: &wgpu::TextureView, streaks: &wgpu::TextureView, output: usize| {
                bloom_steps.push(BloomStep {
                    stage,
                    output,
                    texture_scale: texture_scale(source.size(), bloom_targets[output].size()),
                    pass: self.bloom_pass(device, &source.view, glow, streaks),
                });
            };
        for (level, &[extract, scratch, blur]) in levels.iter().enumerate() {
            match level {
                0 => step(BloomStage::Extract, &scene, black, black, extract),
                _ => step(BloomStage::Downsample, &bloom_targets[levels[level - 1][0]], black, black, extract),
            }
            step(BloomStage::BlurHorizontal, &bloom_targets[extract], black, black, scratch);
            step(BloomStage::BlurVertical, &bloom_targets[scratch], black, black, blur);
        }
        let mut glow = levels.last().map(|level| level[2]);
        for level in (0..levels.len().saturating_sub(1)).rev() {
            let [extract, _, blur] = levels[level];
            let wider = &bloom_targets[glow.expect("a wider level exists")];
            step(BloomStage::Upsample, wider, &bloom_targets[blur].view, black, extract);
            glow = Some(extract);
        }
        if let (Some(lens), Some(glow)) = (lens, glow) {
            step(BloomStage::Lens, &bloom_targets[glow], black, black, lens);
        }
        let glow_view = glow.map_or(black, |glow| &bloom_targets[glow].view);
        let lens_view = lens.map_or(black, |lens| &bloom_targets[lens].view);
        if let Some(composite) = composite {
            step(BloomStage::Composite, &scene, glow_view, lens_view, composite);
        }

        let blur_targets = key.full_blur.then(|| {
            (
                Target::new(device, "blur horizontal", key.width, key.height, format),
                Target::new(device, "blur vertical", key.width, key.height, format),
            )
        });
        let edge_targets = key.edge_blur.then(|| {
            let (width, height) = ((key.width / 2).max(1), (key.height / 2).max(1));
            (
                Target::new(device, "edge horizontal", width, height, format),
                Target::new(device, "edge vertical", width, height, format),
            )
        });

        // Later passes read the chain so far, as the TypeScript `previous` texture: the bloomed scene when bloom
        // composed early, then the full-image blur.
        let mut chain = composite.map_or(&scene.view, |composite| &bloom_targets[composite].view);
        let blur_passes = match &blur_targets {
            Some((horizontal, vertical)) => {
                let passes = vec![self.blur_pass(device, chain), self.blur_pass(device, &horizontal.view)];
                chain = &vertical.view;
                passes
            }
            None => Vec::new(),
        };
        let edge_passes = match &edge_targets {
            Some((horizontal, _)) => vec![self.blur_pass(device, chain), self.blur_pass(device, &horizontal.view)],
            None => Vec::new(),
        };

        // The resolve adds the glow itself unless it was composed early.
        let (resolve_glow, resolve_lens) = if composite.is_some() { (black, black) } else { (glow_view, lens_view) };
        let edge_view = edge_targets.as_ref().map_or(black, |(_, vertical)| &vertical.view);
        let resolve = self.resolve_pass(device, chain, resolve_glow, resolve_lens, edge_view);
        let presentation = self.presentation_pass(device, &display.view);

        Graph {
            key,
            scene,
            depth,
            display,
            bloom_targets,
            bloom_steps,
            blur_targets,
            edge_targets,
            blur_passes,
            edge_passes,
            resolve,
            presentation,
        }
    }

    /// Upload uniforms derived from the current settings and drawing size.
    fn write_uniforms(&self, queue: &wgpu::Queue, time_seconds: f32) {
        let graph = self.graph.as_ref().expect("graph exists after prepare");
        let config = &self.config;
        let shortest_axis = graph.key.width.min(graph.key.height) as f32;

        // Bloom: the TypeScript renderer's construction options and per-frame `createEffectUniforms` values.
        let (horizontal_radius, vertical_radius) = bloom_blur_radii(config, shortest_axis);
        let bloom = BloomUniforms {
            threshold: config.bloom_threshold,
            soft_knee: config.bloom_knee / config.bloom_threshold.max(0.00001),
            exposure: config.exposure,
            scatter: config.bloom_scatter,
            intensity: config.bloom_strength,
            starburst_intensity: config.bloom_lens_flare,
            starburst_length: (config.bloom_radius * shortest_axis * STARBURST_LENGTH_SCALE)
                .clamp(0.0, MAX_STARBURST_LENGTH),
            ..BloomUniforms::default()
        };
        for step in &graph.bloom_steps {
            let (delta, radius) = match step.stage {
                BloomStage::BlurHorizontal => ([1.0, 0.0], horizontal_radius),
                BloomStage::BlurVertical => ([0.0, 1.0], vertical_radius),
                _ => ([0.0, 0.0], 0.0),
            };
            let uniforms = BloomUniforms { texture_scale: step.texture_scale, delta, radius, ..bloom };
            queue.write_buffer(&step.pass.uniforms, 0, bytemuck::bytes_of(&uniforms));
        }

        let blur_radius = (config.blur_radius * shortest_axis).min(MAX_BLUR_RADIUS);
        for (index, pass) in graph.blur_passes.iter().enumerate() {
            let direction =
                if index == 0 { [1.0 / graph.key.width as f32, 0.0] } else { [0.0, 1.0 / graph.key.height as f32] };
            let uniforms = BlurUniforms { direction, radius: blur_radius, padding: 0.0 };
            queue.write_buffer(&pass.uniforms, 0, bytemuck::bytes_of(&uniforms));
        }
        if let Some((horizontal, _)) = &graph.edge_targets {
            for (index, pass) in graph.edge_passes.iter().enumerate() {
                let direction = if index == 0 {
                    [1.0 / horizontal.width as f32, 0.0]
                } else {
                    [0.0, 1.0 / horizontal.height as f32]
                };
                let uniforms = BlurUniforms { direction, radius: EDGE_BLUR_RADIUS, padding: 0.0 };
                queue.write_buffer(&pass.uniforms, 0, bytemuck::bytes_of(&uniforms));
            }
        }

        // Grade adjustments use the same conversions as the TypeScript effect uniforms.
        let contrast = if config.contrast > 1.0 { 1.0 - (1.0 / config.contrast) } else { config.contrast - 1.0 };
        let saturation =
            if config.saturation > 1.0 { 1.001 - (1.0 / config.saturation) } else { config.saturation - 1.0 };
        let tone_mapper = match config.tone_mapper {
            ToneMapper::None => 0.0,
            ToneMapper::Aces => 1.0,
            ToneMapper::Agx => 2.0,
        };
        let look = if config.agx_look == AgxLook::Punchy { 1.0 } else { 0.0 };
        let resolve_bloom = graph.key.bloom_levels > 0 && !graph.key.composes_bloom_early();
        let bloom_strength = if resolve_bloom { config.bloom_strength } else { 0.0 };
        let resolve = ResolveUniforms {
            display: [config.exposure, tone_mapper, look, bloom_strength],
            grade: [saturation, config.brightness - 1.0, contrast, config.vibrance],
            vignette: [config.vignette_radius, config.vignette_strength, config.vignette_blur, 0.0],
            viewport: [graph.key.width as f32, graph.key.height as f32, time_seconds, 0.0],
        };
        queue.write_buffer(&graph.resolve.uniforms, 0, bytemuck::bytes_of(&resolve));

        let decorrelated = if config.film_grain_noise == FilmGrainNoise::Decorrelated { 1.0 } else { 0.0 };
        let presentation = PresentationUniforms {
            frame: [
                1.0 / graph.key.width as f32,
                1.0 / graph.key.height as f32,
                config.antialias_strength,
                config.film_grain,
            ],
            grain: [config.film_grain_size, config.film_grain_speed, decorrelated, time_seconds],
        };
        queue.write_buffer(&graph.presentation.uniforms, 0, bytemuck::bytes_of(&presentation));
    }

    fn uniform_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn bloom_pass(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        glow: &wgpu::TextureView,
        lens: &wgpu::TextureView,
    ) -> Pass {
        let uniforms = Self::uniform_buffer(device, "bloom uniforms", std::mem::size_of::<BloomUniforms>());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom"),
            layout: &self.bloom_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.linear_sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(glow) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(lens) },
            ],
        });
        Pass { bind_group, uniforms }
    }

    fn blur_pass(&self, device: &wgpu::Device, source: &wgpu::TextureView) -> Pass {
        let uniforms = Self::uniform_buffer(device, "blur uniforms", std::mem::size_of::<BlurUniforms>());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.linear_sampler) },
            ],
        });
        Pass { bind_group, uniforms }
    }

    fn resolve_pass(
        &self,
        device: &wgpu::Device,
        scene: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
        lens: &wgpu::TextureView,
        edge: &wgpu::TextureView,
    ) -> Pass {
        let uniforms = Self::uniform_buffer(device, "resolve uniforms", std::mem::size_of::<ResolveUniforms>());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resolve"),
            layout: &self.resolve_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(scene) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(bloom) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(edge) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.linear_sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(lens) },
            ],
        });
        Pass { bind_group, uniforms }
    }

    fn presentation_pass(&self, device: &wgpu::Device, display: &wgpu::TextureView) -> Pass {
        let uniforms =
            Self::uniform_buffer(device, "presentation uniforms", std::mem::size_of::<PresentationUniforms>());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("presentation"),
            layout: &self.presentation_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(display) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.linear_sampler) },
            ],
        });
        Pass { bind_group, uniforms }
    }
}

/// A pyramid level's extent along one axis: luma.gl rounds each scale of the drawing buffer half up, at least one.
fn level_size(size: u32, scale: f64) -> u32 {
    ((f64::from(size) * scale).round() as u32).max(1)
}

/// luma.gl's `calculateTextureScale`: aspect correction between a source and its output.
fn texture_scale(source: (u32, u32), output: (u32, u32)) -> [f32; 2] {
    let source_aspect = f64::from(source.0) / f64::from(source.1.max(1));
    let output_aspect = f64::from(output.0) / f64::from(output.1.max(1));
    if output_aspect > source_aspect {
        [1.0, (output_aspect / source_aspect) as f32]
    } else {
        [(source_aspect / output_aspect) as f32, 1.0]
    }
}

/// Horizontal and vertical Gaussian radii in level texels: the TypeScript kernel scale, then luma.gl's anamorphic
/// stretch, which lengthens one axis while keeping it within the blur limit.
fn bloom_blur_radii(config: &PostProcessingConfig, shortest_axis: f32) -> (f32, f32) {
    let radius = (config.bloom_radius * shortest_axis * BLOOM_RADIUS_SCALE).min(MAX_BLOOM_RADIUS);
    let ratio = config.bloom_anamorphic.clamp(-1.0, 1.0);
    let anamorphic_radius = radius.min(MAX_BLOOM_BLUR_RADIUS / (1.0 + ratio.abs()));
    (anamorphic_radius * (1.0 + ratio.max(0.0)), anamorphic_radius * (1.0 + (-ratio).max(0.0)))
}

/// Record one full-screen triangle into `target`.
fn fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    pass: &Pass,
) {
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            depth_slice: None,
        })],
        ..Default::default()
    });
    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, &pass.bind_group, &[]);
    render_pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::post_processing::BloomQuality;

    #[test]
    fn pyramid_levels_round_like_luma() {
        // 1366 × 768: a quarter of 1366 is 341.5, which JavaScript's Math.round takes up.
        let sizes: Vec<(u32, u32)> =
            BLOOM_LEVEL_SCALES.iter().map(|scale| (level_size(1366, *scale), level_size(768, *scale))).collect();
        assert_eq!(sizes, [(683, 384), (342, 192), (171, 96), (85, 48), (43, 24)]);
        assert_eq!(level_size(1, 0.03125), 1);
    }

    #[test]
    fn texture_scale_corrects_aspect_like_luma() {
        assert_eq!(texture_scale((1920, 1080), (960, 540)), [1.0, 1.0]);
        assert_eq!(texture_scale((100, 100), (200, 100)), [1.0, 2.0]);
        assert_eq!(texture_scale((200, 100), (100, 100)), [2.0, 1.0]);
    }

    #[test]
    fn anamorphic_stretch_keeps_each_axis_within_the_blur_limit() {
        let config = |radius: f32, anamorphic: f32| PostProcessingConfig {
            bloom_radius: radius,
            bloom_anamorphic: anamorphic,
            ..PostProcessingConfig::default()
        };
        // 0.025 of 720 pixels is eight texels; a ratio of 0.5 stretches the horizontal axis to twelve.
        let (horizontal, vertical) = bloom_blur_radii(&config(0.025, 0.5), 720.0);
        assert!((horizontal - 12.0).abs() < 1e-4 && (vertical - 8.0).abs() < 1e-4, "{horizontal} {vertical}");
        // At the 24-texel cap the stretched axis stays at 24 and the other shrinks to 16.
        let (horizontal, vertical) = bloom_blur_radii(&config(0.25, 0.5), 720.0);
        assert!((horizontal - 24.0).abs() < 1e-4 && (vertical - 16.0).abs() < 1e-4, "{horizontal} {vertical}");
        assert_eq!(bloom_blur_radii(&config(0.0, 1.0), 720.0), (0.0, 0.0));
    }

    #[test]
    fn graph_shape_follows_only_typescript_pipeline_key_values() {
        let base = PostProcessingConfig { bloom_strength: 1.0, ..PostProcessingConfig::default() };
        let key = |config: &PostProcessingConfig| GraphKey::new(config, 640, 360);
        // Uniform-only changes keep the graph.
        let tuned = PostProcessingConfig {
            bloom_strength: 2.5,
            bloom_threshold: 0.4,
            bloom_knee: 0.9,
            bloom_scatter: 0.1,
            bloom_radius: 0.08,
            bloom_anamorphic: 0.7,
            exposure: 1.8,
            ..base.clone()
        };
        assert_eq!(key(&base), key(&tuned));
        let streaks = PostProcessingConfig { bloom_lens_flare: 0.5, ..base.clone() };
        assert_eq!(key(&streaks), key(&PostProcessingConfig { bloom_lens_flare: 2.0, ..streaks.clone() }));
        // Bloom on/off, quality, and streaks on/off reshape it.
        assert_ne!(key(&base), key(&streaks));
        assert_ne!(key(&base), key(&PostProcessingConfig { bloom_quality: BloomQuality::Low, ..base.clone() }));
        assert_eq!(key(&PostProcessingConfig { bloom_strength: 0.0, ..streaks.clone() }).bloom_levels, 0);
        assert!(!key(&PostProcessingConfig { bloom_strength: 0.0, ..streaks }).lens_flare);
        // A later blur moves the composite ahead of it.
        assert!(!key(&base).composes_bloom_early());
        assert!(key(&PostProcessingConfig { vignette_blur: 0.5, ..base.clone() }).composes_bloom_early());
        assert!(key(&PostProcessingConfig { blur_radius: 0.01, ..base }).composes_bloom_early());
    }

    /// Every bloom stage and the resolve validate and translate to WebGL 2 GLSL without extensions.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn bloom_and_resolve_translate_for_webgl() {
        use wgpu::naga::back::glsl;
        let stages = [
            (BLOOM_SOURCE, "extractMain"),
            (BLOOM_SOURCE, "downsampleMain"),
            (BLOOM_SOURCE, "blurMain"),
            (BLOOM_SOURCE, "upsampleMain"),
            (BLOOM_SOURCE, "lensMain"),
            (BLOOM_SOURCE, "compositeMain"),
            (RESOLVE_SOURCE, "fragmentMain"),
        ];
        for (source, entry_point) in stages {
            let module =
                wgpu::naga::front::wgsl::parse_str(source).unwrap_or_else(|error| panic!("{entry_point}: {error}"));
            let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|error| panic!("{entry_point}: {error:?}"));
            let options = glsl::Options {
                version: glsl::Version::Embedded { version: 300, is_webgl: true },
                ..Default::default()
            };
            let pipeline = glsl::PipelineOptions {
                shader_stage: wgpu::naga::ShaderStage::Fragment,
                entry_point: entry_point.into(),
                multiview: None,
            };
            let mut output = String::new();
            glsl::Writer::new(&mut output, &module, &info, &options, &pipeline, Default::default())
                .and_then(|mut writer| writer.write().map(|_| ()))
                .unwrap_or_else(|error| panic!("{entry_point}: {error:?}"));
            assert!(!output.contains("#extension"), "{entry_point} needs a GLSL extension");
        }
    }

    #[test]
    fn bloom_uniform_block_matches_the_shader_layout() {
        assert_eq!(std::mem::size_of::<BloomUniforms>(), 48);
        let source = BLOOM_SOURCE;
        let members = source.split("struct Bloom {").nth(1).unwrap().split("};").next().unwrap();
        let names: Vec<&str> = members
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .filter_map(|line| line.split(':').next())
            .collect();
        assert_eq!(
            names,
            [
                "textureScale",
                "delta",
                "threshold",
                "softKnee",
                "exposure",
                "radius",
                "scatter",
                "intensity",
                "starburstIntensity",
                "starburstLength"
            ]
        );
    }
}
