// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Adaptive exposure: meter the scene's luminance, adapt toward the exposure that maps it to the key value, and copy
//! the one-texel result into the display chain's exposure texture.
//!
//! With compute shaders, a luminance histogram meters the scene and percentiles reject small lights and deep shadows
//! (`exposure.wgsl`). Without them (WebGL 2), a center-weighted log average is reduced through small render targets
//! (`exposure-fallback.wgsl`), which float render targets make possible.

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::pipelines::{
    FULLSCREEN_SOURCE,
    FullscreenTarget,
    draw_fullscreen,
    fullscreen_pipeline,
    sampler_entry,
    texture_entry,
    uniform_entry,
};
use crate::scene_effects::config::AdaptiveExposureOptions;
use crate::scene_effects::{
    EffectsFrame,
    effect_target,
    linear_sampler,
};

const COMPUTE_SOURCE: &str = include_str!("shaders/exposure.wgsl");
const FALLBACK_SOURCE: &str = include_str!("shaders/exposure-fallback.wgsl");

/// Format of the metering targets and the one-texel exposure (the display chain's exposure format).
const EXPOSURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Metered log2 luminance range: scene luminance from 2^-12 to 2^12 meters; anything outside clamps to the ends.
const MIN_LOG_LUMINANCE: f32 = -12.0;
const LOG_LUMINANCE_RANGE: f32 = 24.0;

/// Histogram bins (`BIN_COUNT` in exposure.wgsl).
const BIN_COUNT: u64 = 256;

/// Histogram workgroup size on each axis and the pixel stride it meters with (`histogramMain` in exposure.wgsl).
const HISTOGRAM_WORKGROUP: u32 = 16;
const HISTOGRAM_STRIDE: u32 = 2;

/// Fallback metering target sizes (`METER_SIZE` in exposure-fallback.wgsl, then four-to-one reductions).
const METER_SIZES: [u32; 4] = [64, 16, 4, 1];

/// `Exposure` in both exposure shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ExposureUniforms {
    metering: [f32; 4],
    average: [f32; 4],
    limits: [f32; 4],
    speed: [f32; 4],
}

/// Adaptive exposure state for one renderer.
pub(crate) struct AdaptiveExposure {
    uniforms: wgpu::Buffer,
    meter: Meter,
    /// Whether the next frame restarts from the initial exposure.
    restart: bool,
}

/// How exposure is metered on this device.
enum Meter {
    Histogram(HistogramMeter),
    Reduction(ReductionMeter),
}

impl AdaptiveExposure {
    /// Create the metering for this device: the histogram with `compute`, the reduction chain otherwise.
    pub(crate) fn new(device: &wgpu::Device, compute: bool) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("adaptive exposure"),
            size: std::mem::size_of::<ExposureUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let meter = if compute {
            Meter::Histogram(HistogramMeter::new(device))
        } else {
            Meter::Reduction(ReductionMeter::new(device, &uniforms))
        };
        Self {
            uniforms,
            meter,
            restart: true,
        }
    }

    /// Restart from the initial exposure on the next frame.
    pub(crate) fn reset(&mut self) {
        self.restart = true;
    }

    /// Meter the scene color, adapt, and copy the exposure multiplier into `exposure_texture` (one texel of
    /// [`EXPOSURE_FORMAT`], read by the display chain).
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        options: &AdaptiveExposureOptions,
        exposure_texture: &wgpu::Texture,
    ) {
        let restart = self.restart || frame.reset_history;
        let uniforms = ExposureUniforms {
            metering: [MIN_LOG_LUMINANCE, LOG_LUMINANCE_RANGE, options.center_weight, 0.0],
            average: [options.low_percentile, options.high_percentile, options.key_value, frame.delta_seconds],
            limits: [
                options.minimum_exposure,
                options.maximum_exposure,
                options.initial_exposure,
                if restart { 1.0 } else { 0.0 },
            ],
            speed: [options.brighten_speed, options.darken_speed, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        self.restart = false;

        let result = match &mut self.meter {
            Meter::Histogram(meter) => meter.encode(device, encoder, frame, &self.uniforms),
            Meter::Reduction(meter) => meter.encode(device, encoder, frame, &self.uniforms),
        };
        encoder.copy_texture_to_texture(
            result.as_image_copy(),
            exposure_texture.as_image_copy(),
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// Histogram metering with compute shaders.
struct HistogramMeter {
    layout: wgpu::BindGroupLayout,
    histogram: wgpu::ComputePipeline,
    adapt: wgpu::ComputePipeline,
    // Zero-initialized on creation; the adapt pass clears the bins after reading them.
    bins: wgpu::Buffer,
    state: wgpu::Buffer,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    /// The bind group for the scene view it was built against.
    bound: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl HistogramMeter {
    fn new(device: &wgpu::Device) -> Self {
        let storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: false,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("exposure histogram"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ..uniform_entry(0)
                },
                wgpu::BindGroupLayoutEntry {
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ..texture_entry(1)
                },
                storage(2),
                storage(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: EXPOSURE_FORMAT,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("adaptive exposure"),
            source: wgpu::ShaderSource::Wgsl(COMPUTE_SOURCE.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("adaptive exposure"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label: &str, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let histogram = pipeline("exposure histogram", "histogramMain");
        let adapt = pipeline("exposure adapt", "adaptMain");
        let buffer = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let bins = buffer("exposure histogram", BIN_COUNT * 4);
        let state = buffer("exposure state", 16);
        let output_usage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC;
        let output = effect_target(device, "adapted exposure", (1, 1), EXPOSURE_FORMAT, output_usage);
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            layout,
            histogram,
            adapt,
            bins,
            state,
            output,
            output_view,
            bound: None,
        }
    }

    /// Record the histogram and adapt dispatches; returns the one-texel result.
    fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        uniforms: &wgpu::Buffer,
    ) -> &wgpu::Texture {
        let group = self.bind(device, frame.scene_view, uniforms);
        let metered = (frame.size.0.div_ceil(HISTOGRAM_STRIDE), frame.size.1.div_ceil(HISTOGRAM_STRIDE));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("adaptive exposure"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &group, &[]);
        pass.set_pipeline(&self.histogram);
        pass.dispatch_workgroups(metered.0.div_ceil(HISTOGRAM_WORKGROUP), metered.1.div_ceil(HISTOGRAM_WORKGROUP), 1);
        pass.set_pipeline(&self.adapt);
        pass.dispatch_workgroups(1, 1, 1);
        drop(pass);
        &self.output
    }

    fn bind(&mut self, device: &wgpu::Device, scene: &wgpu::TextureView, uniforms: &wgpu::Buffer) -> wgpu::BindGroup {
        if let Some((bound_scene, group)) = &self.bound
            && bound_scene == scene
        {
            return group.clone();
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("exposure histogram"),
            layout: &self.layout,
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
                    resource: self.bins.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.output_view),
                },
            ],
        });
        self.bound = Some((scene.clone(), group.clone()));
        group
    }
}

/// Log-average metering through render-target reductions, for backends without compute.
struct ReductionMeter {
    meter: wgpu::RenderPipeline,
    reduce: wgpu::RenderPipeline,
    adapt: wgpu::RenderPipeline,
    /// The metering targets, from [`METER_SIZES`]`[0]` down to one texel.
    levels: Vec<(wgpu::Texture, wgpu::TextureView)>,
    /// Groups reading each level after the first, for the reductions; built once.
    reduce_groups: Vec<wgpu::BindGroup>,
    /// Exposure ping-pong: each frame adapts from one texel into the other.
    exposures: [(wgpu::Texture, wgpu::TextureView); 2],
    /// Adapt groups reading the last level and exposure `index`.
    adapt_groups: [wgpu::BindGroup; 2],
    current: usize,
    meter_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The metering group for the scene view it was built against.
    bound: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl ReductionMeter {
    fn new(device: &wgpu::Device, uniforms: &wgpu::Buffer) -> Self {
        // Metering and reduction read one source with a sampler; adaptation reads the metered texel and the previous
        // exposure. Each layout holds only the bindings its entry points use.
        let meter_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("exposure meter"),
            entries: &[uniform_entry(0), texture_entry(1), sampler_entry(3)],
        });
        let adapt_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("exposure adapt"),
            entries: &[uniform_entry(0), texture_entry(1), texture_entry(2)],
        });
        let source = format!("{FULLSCREEN_SOURCE}\n{FALLBACK_SOURCE}");
        let pipeline = |label: &str, entry: &str, layout: &wgpu::BindGroupLayout| {
            fullscreen_pipeline(device, label, &source, entry, &[layout], EXPOSURE_FORMAT, wgpu::BlendState::REPLACE)
        };
        let meter = pipeline("exposure meter", "meterMain", &meter_layout);
        let reduce = pipeline("exposure reduce", "reduceMain", &meter_layout);
        let adapt = pipeline("exposure adapt", "adaptMain", &adapt_layout);
        let sampler = linear_sampler(device, "exposure meter");

        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let levels: Vec<(wgpu::Texture, wgpu::TextureView)> = METER_SIZES
            .iter()
            .map(|size| {
                let texture = effect_target(device, "exposure meter", (*size, *size), EXPOSURE_FORMAT, usage);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (texture, view)
            })
            .collect();
        let exposures = [0, 1].map(|_| {
            let texture = effect_target(
                device,
                "adapted exposure",
                (1, 1),
                EXPOSURE_FORMAT,
                usage | wgpu::TextureUsages::COPY_SRC,
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        });

        let meter_group = |source: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("exposure meter"),
                layout: &meter_layout,
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
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        };
        let reduce_groups = levels[..levels.len() - 1].iter().map(|(_, view)| meter_group(view)).collect();
        let metered = &levels[levels.len() - 1].1;
        let adapt_groups = [0, 1].map(|previous| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("exposure adapt"),
                layout: &adapt_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(metered),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&exposures[previous].1),
                    },
                ],
            })
        });
        Self {
            meter,
            reduce,
            adapt,
            levels,
            reduce_groups,
            exposures,
            adapt_groups,
            current: 0,
            meter_layout,
            sampler,
            bound: None,
        }
    }

    /// Record the metering, reductions, and adaptation; returns the one-texel result.
    fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        uniforms: &wgpu::Buffer,
    ) -> &wgpu::Texture {
        let meter_group = self.bind(device, frame.scene_view, uniforms);
        let first = FullscreenTarget::cleared(&self.levels[0].1);
        draw_fullscreen(encoder, "exposure meter", first, &self.meter, &[&meter_group]);
        for (group, (_, target)) in self.reduce_groups.iter().zip(&self.levels[1..]) {
            draw_fullscreen(encoder, "exposure reduce", FullscreenTarget::cleared(target), &self.reduce, &[group]);
        }
        let next = 1 - self.current;
        let target = FullscreenTarget::cleared(&self.exposures[next].1);
        draw_fullscreen(encoder, "exposure adapt", target, &self.adapt, &[&self.adapt_groups[self.current]]);
        self.current = next;
        &self.exposures[next].0
    }

    fn bind(&mut self, device: &wgpu::Device, scene: &wgpu::TextureView, uniforms: &wgpu::Buffer) -> wgpu::BindGroup {
        if let Some((bound_scene, group)) = &self.bound
            && bound_scene == scene
        {
            return group.clone();
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("exposure meter"),
            layout: &self.meter_layout,
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
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.bound = Some((scene.clone(), group.clone()));
        group
    }
}
