// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene-to-display pipeline: HDR scene targets, bloom, blurs, the display resolve, and the final presentation pass.
//!
//! The pass order matches the TypeScript engine: bloom, optional full-image blur and vignette edge blur, exposure and
//! tone mapping, grade, vignette, antialiasing, then film grain. Targets are rebuilt only when the drawing size or the
//! effect graph (bloom levels, blur passes) changes; intensities are plain uniforms.

use bytemuck::{Pod, Zeroable};
use joy_foundation::post_processing::{AgxLook, FilmGrainNoise, PostProcessingConfig, ToneMapper};

use crate::pipelines::{
    BLOOM_SOURCE, BLUR_SOURCE, DEPTH_FORMAT, DISPLAY_FORMAT, FINAL_SOURCE, RESOLVE_SOURCE, fullscreen_pipeline,
    sampler_entry, texture_entry, uniform_entry,
};

/// Maps the authored bloom radius to the TypeScript renderer's kernel scale: 0.025 at 720 pixels is eight texels.
const BLOOM_RADIUS_SCALE: f32 = 8.0 / (720.0 * 0.025);

/// Upper bound on the bloom kernel in texels, matching the TypeScript renderer.
const MAX_BLOOM_RADIUS: f32 = 24.0;

/// Upper bound on the full-image Gaussian radius in texels.
const MAX_BLUR_RADIUS: f32 = 32.0;

/// Edge-blur radius in half-resolution texels, matching the TypeScript vignette blur.
const EDGE_BLUR_RADIUS: f32 = 6.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BloomUniforms {
    source_texel: [f32; 2],
    radius: f32,
    scatter: f32,
    extract: [f32; 4],
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

    fn texel(&self) -> [f32; 2] {
        [1.0 / self.width as f32, 1.0 / self.height as f32]
    }
}

/// Shape of the effect graph; a change rebuilds targets and bind groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GraphKey {
    width: u32,
    height: u32,
    bloom_levels: usize,
    full_blur: bool,
    edge_blur: bool,
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
    bloom_levels: Vec<Target>,
    bloom_upsampled: Vec<Target>,
    blur_targets: Option<(Target, Target)>,
    edge_targets: Option<(Target, Target)>,
    bright_pass: Option<Pass>,
    downsample_passes: Vec<Pass>,
    upsample_passes: Vec<Pass>,
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
    bright_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    upsample_pipeline: wgpu::RenderPipeline,
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
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2), texture_entry(3)],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post blur"),
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2)],
        });
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post resolve"),
            entries: &[uniform_entry(0), texture_entry(1), texture_entry(2), texture_entry(3), sampler_entry(4)],
        });
        let presentation_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post presentation"),
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2)],
        });

        let bright_pipeline =
            fullscreen_pipeline(device, "bloom-bright", BLOOM_SOURCE, "brightMain", &bloom_layout, scene_format);
        let downsample_pipeline = fullscreen_pipeline(
            device,
            "bloom-downsample",
            BLOOM_SOURCE,
            "downsampleMain",
            &bloom_layout,
            scene_format,
        );
        let upsample_pipeline =
            fullscreen_pipeline(device, "bloom-upsample", BLOOM_SOURCE, "upsampleMain", &bloom_layout, scene_format);
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
            bright_pipeline,
            downsample_pipeline,
            upsample_pipeline,
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
        let key = GraphKey {
            width,
            height,
            bloom_levels: if self.config.bloom_strength > 0.0 { self.config.bloom_quality.levels() } else { 0 },
            full_blur: self.config.blur_radius > 0.0,
            edge_blur: self.config.vignette_blur > 0.0,
        };
        if self.graph.as_ref().map(|graph| graph.key) != Some(key) {
            self.graph = Some(self.build_graph(device, key));
        }
        self.write_uniforms(queue, time_seconds);
    }

    /// Record every post-processing pass after the scene pass, ending in `output`.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let graph = self.graph.as_ref().expect("prepare runs before encode");

        // Bloom pyramid: bright pass, downsamples, then upsamples back to the first level.
        if let Some(bright) = &graph.bright_pass {
            fullscreen(encoder, "bloom bright", &graph.bloom_levels[0].view, &self.bright_pipeline, bright);
            for (index, pass) in graph.downsample_passes.iter().enumerate() {
                fullscreen(
                    encoder,
                    "bloom downsample",
                    &graph.bloom_levels[index + 1].view,
                    &self.downsample_pipeline,
                    pass,
                );
            }
            for (index, pass) in graph.upsample_passes.iter().enumerate() {
                let level = graph.upsample_passes.len() - 1 - index;
                fullscreen(
                    encoder,
                    "bloom upsample",
                    &graph.bloom_upsampled[level].view,
                    &self.upsample_pipeline,
                    pass,
                );
            }
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

        // Bloom pyramid levels start at half resolution.
        let mut bloom_levels = Vec::new();
        let (mut level_width, mut level_height) = (key.width, key.height);
        for level in 0..key.bloom_levels {
            level_width = (level_width / 2).max(1);
            level_height = (level_height / 2).max(1);
            bloom_levels.push(Target::new(device, &format!("bloom level {level}"), level_width, level_height, format));
        }
        let bloom_upsampled: Vec<Target> = bloom_levels
            .iter()
            .take(bloom_levels.len().saturating_sub(1))
            .enumerate()
            .map(|(level, target)| {
                Target::new(device, &format!("bloom upsampled {level}"), target.width, target.height, format)
            })
            .collect();

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

        // Bloom passes. The bright pass samples the full-resolution scene; each later pass samples the previous level.
        let bright_pass = bloom_levels.first().map(|_| self.bloom_pass(device, &scene.view, &self.black.view));
        let downsample_passes = (1..bloom_levels.len())
            .map(|level| {
                let source = &bloom_levels[level - 1];
                self.bloom_pass(device, &source.view, &self.black.view)
            })
            .collect();
        // Upsamples run from the widest level inward: each reads the wider result and this level's detail.
        let mut upsample_passes = Vec::new();
        for level in (0..bloom_upsampled.len()).rev() {
            let wider =
                if level + 1 == bloom_upsampled.len() { &bloom_levels[level + 1] } else { &bloom_upsampled[level + 1] };
            upsample_passes.push(self.bloom_pass(device, &wider.view, &bloom_levels[level].view));
        }

        let blur_passes = match &blur_targets {
            Some((horizontal, _)) => {
                vec![self.blur_pass(device, &scene.view), self.blur_pass(device, &horizontal.view)]
            }
            None => Vec::new(),
        };
        let edge_passes = match &edge_targets {
            Some((horizontal, _)) => {
                vec![self.blur_pass(device, &scene.view), self.blur_pass(device, &horizontal.view)]
            }
            None => Vec::new(),
        };

        // The resolve reads the blurred scene when the full blur is active.
        let resolve_scene = blur_targets.as_ref().map_or(&scene.view, |(_, vertical)| &vertical.view);
        let bloom_view = match (bloom_upsampled.first(), bloom_levels.first()) {
            (Some(upsampled), _) => &upsampled.view,
            (None, Some(level)) => &level.view,
            (None, None) => &self.black.view,
        };
        let edge_view = edge_targets.as_ref().map_or(&self.black.view, |(_, vertical)| &vertical.view);
        let resolve = self.resolve_pass(device, resolve_scene, bloom_view, edge_view);
        let presentation = self.presentation_pass(device, &display.view);

        Graph {
            key,
            scene,
            depth,
            display,
            bloom_levels,
            bloom_upsampled,
            blur_targets,
            edge_targets,
            bright_pass,
            downsample_passes,
            upsample_passes,
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

        // Bloom extraction uses exposed color, like the TypeScript bloom extract.
        let bloom_radius = (config.bloom_radius * shortest_axis * BLOOM_RADIUS_SCALE).min(MAX_BLOOM_RADIUS);
        let extract = [config.bloom_threshold, config.bloom_knee, config.exposure, config.bloom_anamorphic];
        let pyramid_radius = (bloom_radius / 8.0).max(1.0);
        if let Some(pass) = &graph.bright_pass {
            let uniforms = BloomUniforms {
                source_texel: graph.scene.texel(),
                radius: pyramid_radius,
                scatter: config.bloom_scatter,
                extract,
            };
            queue.write_buffer(&pass.uniforms, 0, bytemuck::bytes_of(&uniforms));
        }
        for (index, pass) in graph.downsample_passes.iter().enumerate() {
            let uniforms = BloomUniforms {
                source_texel: graph.bloom_levels[index].texel(),
                radius: pyramid_radius,
                scatter: config.bloom_scatter,
                extract,
            };
            queue.write_buffer(&pass.uniforms, 0, bytemuck::bytes_of(&uniforms));
        }
        for (index, pass) in graph.upsample_passes.iter().enumerate() {
            let level = graph.upsample_passes.len() - 1 - index;
            let wider = if level + 1 == graph.bloom_upsampled.len() {
                &graph.bloom_levels[level + 1]
            } else {
                &graph.bloom_upsampled[level + 1]
            };
            let uniforms = BloomUniforms {
                source_texel: wider.texel(),
                radius: pyramid_radius,
                scatter: config.bloom_scatter,
                extract,
            };
            queue.write_buffer(&pass.uniforms, 0, bytemuck::bytes_of(&uniforms));
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
        let bloom_strength = if graph.key.bloom_levels > 0 { config.bloom_strength } else { 0.0 };
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

    fn bloom_pass(&self, device: &wgpu::Device, source: &wgpu::TextureView, detail: &wgpu::TextureView) -> Pass {
        let uniforms = Self::uniform_buffer(device, "bloom uniforms", std::mem::size_of::<BloomUniforms>());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom"),
            layout: &self.bloom_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.linear_sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(detail) },
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
