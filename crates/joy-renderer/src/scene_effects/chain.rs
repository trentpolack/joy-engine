// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runs effect pipelines over a scene texture, following luma.gl's `ShaderPassRenderer` (MIT License, Copyright (c)
//! vis.gl contributors): the source is seeded into a pair of swap textures, each step draws a full-screen pass into the
//! next swap texture (the running "previous" result) or a named target, and history targets keep last frame's value
//! until a step rewrites them, swapping only after the whole frame records.

use std::collections::HashMap;

use crate::scene_effects::pass::{
    Binding,
    BindingKind,
    PassDefinition,
    TEXTURE_TRANSFORM,
    UniformValues,
    assemble,
    find_pass,
    pack_uniforms,
};
use crate::scene_effects::pipelines::{
    HistoryStart,
    InputSource,
    PipelineSpec,
    StepSpec,
    TargetLifetime,
    TargetSampling,
    TargetSpec,
};

/// Textures and buffers the renderer supplies by binding name.
#[derive(Clone, Copy, Default)]
pub(crate) struct ExternalBindings<'a> {
    pub(crate) depth: Option<&'a wgpu::TextureView>,
    pub(crate) normal: Option<&'a wgpu::TextureView>,
    pub(crate) velocity: Option<&'a wgpu::TextureView>,
    pub(crate) point_lights: Option<&'a wgpu::Buffer>,
    pub(crate) cluster_light_counts: Option<&'a wgpu::Buffer>,
    pub(crate) cluster_light_indices: Option<&'a wgpu::Buffer>,
}

/// An owned texture and its view.
struct ChainTexture {
    // The view references the texture; drop it first.
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    width: u32,
    height: u32,
}

impl ChainTexture {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view, _texture: texture, width, height }
    }
}

/// A named target with its optional history copy.
struct ManagedTarget {
    spec: TargetSpec,
    current: ChainTexture,
    history: Option<ChainTexture>,
    history_initialized: bool,
    written: bool,
}

impl ManagedTarget {
    fn new(device: &wgpu::Device, spec: &TargetSpec, size: (u32, u32)) -> Self {
        let width = ((size.0 as f32 * spec.scale).round() as u32).max(1);
        let height = ((size.1 as f32 * spec.scale).round() as u32).max(1);
        let current = ChainTexture::new(device, spec.name, width, height, spec.format);
        let history = matches!(spec.lifetime, TargetLifetime::History(_))
            .then(|| ChainTexture::new(device, spec.name, width, height, spec.format));
        Self { spec: spec.clone(), current, history, history_initialized: false, written: false }
    }

    /// The texture a read sees: last frame's value for history targets not yet written this frame.
    fn readable(&self) -> &ChainTexture {
        match &self.history {
            Some(history) if !self.written => history,
            _ => &self.current,
        }
    }
}

/// One step's compiled pass.
struct Execution {
    step: StepSpec,
    pass: &'static PassDefinition,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    bindings: Vec<Binding>,
    uniforms: Option<wgpu::Buffer>,
    transform: wgpu::Buffer,
}

/// One pipeline's targets and steps.
struct PipelineRuntime {
    spec: PipelineSpec,
    targets: Vec<ManagedTarget>,
    executions: Vec<Execution>,
}

impl PipelineRuntime {
    fn target(&self, name: &str) -> Option<&ManagedTarget> {
        self.targets.iter().find(|target| target.spec.name == name)
    }
}

/// Samplers and placeholder resources shared by every execution.
pub(crate) struct ChainResources {
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    placeholder_color: ChainTexture,
    placeholder_depth_view: wgpu::TextureView,
    _placeholder_depth: wgpu::Texture,
    placeholder_storage: wgpu::Buffer,
    blitter: Blitter,
}

impl ChainResources {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let sampler = |label, filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let placeholder_depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene effects depth placeholder"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            linear: sampler("scene effects linear", wgpu::FilterMode::Linear),
            nearest: sampler("scene effects nearest", wgpu::FilterMode::Nearest),
            placeholder_color: ChainTexture::new(
                device,
                "scene effects placeholder",
                1,
                1,
                wgpu::TextureFormat::Rgba16Float,
            ),
            placeholder_depth_view: placeholder_depth.create_view(&wgpu::TextureViewDescriptor::default()),
            _placeholder_depth: placeholder_depth,
            placeholder_storage: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene effects storage placeholder"),
                size: 64,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            blitter: Blitter::new(device),
        }
    }

    /// Draw `source` into `target` unchanged (format conversion included).
    pub(crate) fn blit(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) {
        self.blitter.blit(device, encoder, source, target, format, &self.nearest);
    }
}

/// A copy pass per output format.
struct Blitter {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    bindings: Vec<Binding>,
    transform: wgpu::Buffer,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl Blitter {
    fn new(device: &wgpu::Device) -> Self {
        let pass = find_pass("advancedCopy").expect("the copy pass is part of the generated table");
        let (source, bindings) = assemble(pass);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene effects blit"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let entries: Vec<wgpu::BindGroupLayoutEntry> =
            bindings.iter().map(|binding| layout_entry(binding, false, false)).collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene effects blit"),
            entries: &entries,
        });
        let transform = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("scene effects blit transform"),
                contents: &transform_bytes([1.0, 1.0]),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        Self { shader, layout, bindings, transform, pipelines: HashMap::new() }
    }

    fn blit(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        sampler: &wgpu::Sampler,
    ) {
        let (shader, layout) = (&self.shader, &self.layout);
        let pipeline = self
            .pipelines
            .entry(format)
            .or_insert_with(|| full_screen_pipeline(device, "blit", shader, layout, format));
        let entries: Vec<wgpu::BindGroupEntry<'_>> = self
            .bindings
            .iter()
            .map(|binding| wgpu::BindGroupEntry {
                binding: binding.index,
                resource: match binding.kind {
                    BindingKind::Uniform => self.transform.as_entire_binding(),
                    BindingKind::Sampler => wgpu::BindingResource::Sampler(sampler),
                    _ => wgpu::BindingResource::TextureView(source),
                },
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene effects blit"),
            layout,
            entries: &entries,
        });
        draw(encoder, "scene effects blit", target, pipeline, &group);
    }
}

/// Runs one group of pipelines (lighting, temporal, or finishing).
pub(crate) struct ShaderPassChain {
    pipelines: Vec<PipelineRuntime>,
    color_format: wgpu::TextureFormat,
    size: (u32, u32),
    swap: [ChainTexture; 2],
}

impl ShaderPassChain {
    /// Compile every step; targets are sized on the first [`encode`](Self::encode).
    pub(crate) fn new(device: &wgpu::Device, specs: &[PipelineSpec], color_format: wgpu::TextureFormat) -> Self {
        let pipelines = specs
            .iter()
            .map(|spec| {
                let executions = spec.steps.iter().map(|step| compile(device, spec, step, color_format)).collect();
                PipelineRuntime { spec: spec.clone(), targets: Vec::new(), executions }
            })
            .collect();
        let swap = [
            ChainTexture::new(device, "scene effects swap", 1, 1, color_format),
            ChainTexture::new(device, "scene effects swap", 1, 1, color_format),
        ];
        Self { pipelines, color_format, size: (0, 0), swap }
    }

    /// Forget every history so the next frame starts from each target's initial value.
    pub(crate) fn reset_history(&mut self) {
        for target in self.pipelines.iter_mut().flat_map(|pipeline| pipeline.targets.iter_mut()) {
            target.history_initialized = false;
            target.written = false;
        }
    }

    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.size == size {
            return;
        }
        self.size = size;
        self.swap = [
            ChainTexture::new(device, "scene effects swap", size.0, size.1, self.color_format),
            ChainTexture::new(device, "scene effects swap", size.0, size.1, self.color_format),
        ];
        for pipeline in &mut self.pipelines {
            pipeline.targets =
                pipeline.spec.targets.iter().map(|spec| ManagedTarget::new(device, spec, size)).collect();
        }
    }

    /// Record every step over `source` (sized `size`) and return which swap texture holds the result.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut ChainResources,
        source: &wgpu::TextureView,
        size: (u32, u32),
        externals: &ExternalBindings<'_>,
        uniforms: &HashMap<&'static str, UniformValues>,
    ) -> &wgpu::TextureView {
        self.resize(device, size);
        resources.blit(device, encoder, source, &self.swap[0].view, self.color_format);
        let mut previous = 0_usize;

        for pipeline_index in 0..self.pipelines.len() {
            // History targets start from the original scene or their clear color.
            for target in self.pipelines[pipeline_index].targets.iter_mut() {
                let (TargetLifetime::History(start), Some(history)) = (target.spec.lifetime, &target.history) else {
                    continue;
                };
                if target.history_initialized {
                    continue;
                }
                match start {
                    HistoryStart::Original => {
                        resources.blit(device, encoder, source, &history.view, target.spec.format)
                    }
                    HistoryStart::Clear(color) => clear(encoder, &history.view, color),
                }
                target.history_initialized = true;
            }

            for execution_index in 0..self.pipelines[pipeline_index].executions.len() {
                let pipeline = &self.pipelines[pipeline_index];
                let execution = &pipeline.executions[execution_index];
                let output_name = execution.step.output;
                let output_swap = 1 - previous;
                let (output_view, output_size) = match output_name {
                    None => {
                        (&self.swap[output_swap].view, (self.swap[output_swap].width, self.swap[output_swap].height))
                    }
                    Some(name) => {
                        let target = pipeline.target(name).expect("validated step output");
                        (&target.current.view, (target.current.width, target.current.height))
                    }
                };

                // Resolve texture inputs to views and samplers.
                let resolve = |input: InputSource| -> (&wgpu::TextureView, (u32, u32), &wgpu::Sampler) {
                    match input {
                        InputSource::Previous => {
                            let texture = &self.swap[previous];
                            (&texture.view, (texture.width, texture.height), &resources.nearest)
                        }
                        InputSource::Target(name) => {
                            let target = pipeline.target(name).expect("validated step input").readable();
                            let sampling = pipeline.target(name).map(|target| target.spec.sampling);
                            let sampler = if sampling == Some(TargetSampling::Nearest) {
                                &resources.nearest
                            } else {
                                &resources.linear
                            };
                            (&target.view, (target.width, target.height), sampler)
                        }
                    }
                };
                let input_for = |name: &str| -> Option<InputSource> {
                    execution.step.inputs.iter().find(|(binding, _)| *binding == name).map(|(_, source)| *source)
                };
                let source_input = input_for("sourceTexture").unwrap_or(InputSource::Previous);
                let source_size = resolve(source_input).1;

                // Uniform blocks: the texture transform and the pass's own values.
                queue.write_buffer(&execution.transform, 0, &transform_bytes(texture_scale(source_size, output_size)));
                if let Some(buffer) = &execution.uniforms {
                    let empty = UniformValues::new();
                    let runtime = uniforms.get(execution.pass.name).unwrap_or(&empty);
                    queue.write_buffer(buffer, 0, &pack_uniforms(execution.pass, &[&execution.step.uniforms, runtime]));
                }

                let mut views: HashMap<&str, (&wgpu::TextureView, &wgpu::Sampler)> = HashMap::new();
                for binding in &execution.bindings {
                    if !matches!(binding.kind, BindingKind::Texture | BindingKind::DepthTexture) {
                        continue;
                    }
                    let resolved = match (input_for(&binding.name), binding.name.as_str()) {
                        (Some(input), _) => {
                            let (view, _, sampler) = resolve(input);
                            (view, sampler)
                        }
                        (None, "sourceTexture") => {
                            let (view, _, sampler) = resolve(InputSource::Previous);
                            (view, sampler)
                        }
                        (None, "depthTexture") => {
                            (externals.depth.unwrap_or(&resources.placeholder_depth_view), &resources.nearest)
                        }
                        (None, "normalTexture") => {
                            (externals.normal.unwrap_or(&resources.placeholder_color.view), &resources.nearest)
                        }
                        (None, "velocityTexture") => {
                            (externals.velocity.unwrap_or(&resources.placeholder_color.view), &resources.linear)
                        }
                        (None, _) if binding.kind == BindingKind::DepthTexture => {
                            (&resources.placeholder_depth_view, &resources.nearest)
                        }
                        (None, _) => (&resources.placeholder_color.view, &resources.nearest),
                    };
                    views.insert(binding.name.as_str(), resolved);
                }

                let entries: Vec<wgpu::BindGroupEntry<'_>> = execution
                    .bindings
                    .iter()
                    .map(|binding| wgpu::BindGroupEntry {
                        binding: binding.index,
                        resource: match binding.kind {
                            BindingKind::Uniform if binding.name == TEXTURE_TRANSFORM => {
                                execution.transform.as_entire_binding()
                            }
                            BindingKind::Uniform => execution
                                .uniforms
                                .as_ref()
                                .expect("passes with a uniform block own a buffer")
                                .as_entire_binding(),
                            BindingKind::Texture | BindingKind::DepthTexture => {
                                wgpu::BindingResource::TextureView(views[binding.name.as_str()].0)
                            }
                            BindingKind::Sampler => {
                                let texture = binding.name.strip_suffix("Sampler").unwrap_or_default();
                                let sampler = views.get(texture).map_or(&resources.nearest, |(_, sampler)| sampler);
                                // Depth textures and 32-bit float targets require a non-filtering sampler.
                                wgpu::BindingResource::Sampler(if non_filtering(execution, texture, pipeline) {
                                    &resources.nearest
                                } else {
                                    sampler
                                })
                            }
                            BindingKind::Storage => match binding.name.as_str() {
                                "pointLights" => externals.point_lights,
                                "clusterLightCounts" => externals.cluster_light_counts,
                                "clusterLightIndices" => externals.cluster_light_indices,
                                _ => None,
                            }
                            .unwrap_or(&resources.placeholder_storage)
                            .as_entire_binding(),
                        },
                    })
                    .collect();
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(execution.pass.name),
                    layout: &execution.layout,
                    entries: &entries,
                });
                draw(encoder, execution.pass.name, output_view, &execution.pipeline, &group);

                match output_name {
                    None => previous = output_swap,
                    Some(name) => {
                        if let Some(target) =
                            self.pipelines[pipeline_index].targets.iter_mut().find(|target| target.spec.name == name)
                        {
                            target.written = true;
                        }
                    }
                }
            }
        }

        // The frame recorded: written history targets become next frame's history.
        for target in self.pipelines.iter_mut().flat_map(|pipeline| pipeline.targets.iter_mut()) {
            if target.written
                && let Some(history) = target.history.as_mut()
            {
                std::mem::swap(history, &mut target.current);
            }
            target.written = false;
        }
        &self.swap[previous].view
    }
}

/// Whether a texture binding must use a non-filtering sampler in this execution.
fn non_filtering(execution: &Execution, texture: &str, pipeline: &PipelineRuntime) -> bool {
    execution.bindings.iter().any(|binding| binding.name == texture && binding.kind == BindingKind::DepthTexture)
        || unfilterable_input(&execution.step, texture, &pipeline.spec)
}

/// Whether a texture binding reads a 32-bit float target.
fn unfilterable_input(step: &StepSpec, texture: &str, spec: &PipelineSpec) -> bool {
    step.inputs.iter().any(|(binding, source)| {
        *binding == texture
            && matches!(source, InputSource::Target(name) if spec.targets.iter().any(|target| target.name == *name && target.format == wgpu::TextureFormat::Rgba32Float))
    })
}

/// Compile one step's pass for its output format.
fn compile(
    device: &wgpu::Device,
    spec: &PipelineSpec,
    step: &StepSpec,
    color_format: wgpu::TextureFormat,
) -> Execution {
    let pass = find_pass(step.pass).expect("pipelines only name generated passes");
    let (source, bindings) = assemble(pass);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(pass.name),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
        .iter()
        .map(|binding| {
            let texture = binding.name.strip_suffix("Sampler").unwrap_or(&binding.name);
            let depth_pair =
                bindings.iter().any(|other| other.name == texture && other.kind == BindingKind::DepthTexture);
            let unfilterable = unfilterable_input(step, texture, spec);
            layout_entry(binding, unfilterable, depth_pair)
        })
        .collect();
    let layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(pass.name), entries: &entries });
    let output_format = step
        .output
        .and_then(|name| spec.targets.iter().find(|target| target.name == name))
        .map_or(color_format, |target| target.format);
    let pipeline = full_screen_pipeline(device, pass.name, &shader, &layout, output_format);
    let uniforms = (!pass.uniforms.is_empty()).then(|| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(pass.name),
            size: crate::scene_effects::pass::uniform_layout(pass.uniforms).size as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });
    let transform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene effects texture transform"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Execution { step: step.clone(), pass, pipeline, layout, bindings, uniforms, transform }
}

/// A bind group layout entry for a parsed binding.
fn layout_entry(binding: &Binding, unfilterable: bool, depth_pair: bool) -> wgpu::BindGroupLayoutEntry {
    let ty = match binding.kind {
        BindingKind::Uniform => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingKind::Storage => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingKind::Texture => wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: !unfilterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        BindingKind::DepthTexture => wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        BindingKind::Sampler => wgpu::BindingType::Sampler(if unfilterable || depth_pair {
            wgpu::SamplerBindingType::NonFiltering
        } else {
            wgpu::SamplerBindingType::Filtering
        }),
    };
    wgpu::BindGroupLayoutEntry { binding: binding.index, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None }
}

/// A full-screen triangle-strip pipeline writing one color target without blending or depth.
fn full_screen_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Draw the full-screen strip into `target`, clearing it first like luma.gl.
fn draw(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            depth_slice: None,
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..4, 0..1);
}

/// Clear a texture to a color.
fn clear(encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, color: [f32; 4]) {
    let [r, g, b, a] = color.map(f64::from);
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("scene effects history clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        ..Default::default()
    });
}

/// luma.gl's `calculateTextureScale`: aspect correction between a source and its output.
fn texture_scale(source: (u32, u32), output: (u32, u32)) -> [f32; 2] {
    let source_aspect = source.0 as f32 / source.1.max(1) as f32;
    let output_aspect = output.0 as f32 / output.1.max(1) as f32;
    if output_aspect > source_aspect {
        [1.0, output_aspect / source_aspect]
    } else {
        [source_aspect / output_aspect, 1.0]
    }
}

/// The texture-transform block: scale, then a set `flipY` for top-left texture origins (every wgpu backend).
fn transform_bytes(scale: [f32; 2]) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes[0..4].copy_from_slice(&scale[0].to_le_bytes());
    bytes[4..8].copy_from_slice(&scale[1].to_le_bytes());
    bytes[8..12].copy_from_slice(&1_i32.to_le_bytes());
    bytes
}

#[cfg(test)]
#[path = "chain.test.rs"]
mod tests;
