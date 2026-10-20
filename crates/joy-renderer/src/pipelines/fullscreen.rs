// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Full-screen passes: one triangle covering the target, for post-processing and scene effects. Shaders supply a
//! `vertexMain` with no vertex buffers (effect shaders compose `fullscreen.wgsl` for it) and a fragment entry point.

/// The shared full-screen vertex stage and its varying (`FullscreenVarying`, with top-left texture coordinates).
pub(crate) const FULLSCREEN_SOURCE: &str = include_str!("../shaders/fullscreen.wgsl");

/// Compile a full-screen pipeline over `layouts` with a single color target.
pub(crate) fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    fragment_entry: &str,
    layouts: &[&wgpu::BindGroupLayout],
    format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
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
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// How a full-screen draw treats its target and blends into it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FullscreenTarget<'a> {
    pub view: &'a wgpu::TextureView,

    /// Clear first, or keep (for blending pipelines).
    pub load: wgpu::LoadOp<wgpu::Color>,

    /// The blend constant, for pipelines that blend by it.
    pub blend_constant: Option<wgpu::Color>,
}

impl<'a> FullscreenTarget<'a> {
    /// A target cleared to black before the draw.
    pub fn cleared(view: &'a wgpu::TextureView) -> Self {
        Self {
            view,
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            blend_constant: None,
        }
    }

    /// A target whose contents the draw blends into.
    pub fn blended(view: &'a wgpu::TextureView) -> Self {
        Self {
            view,
            load: wgpu::LoadOp::Load,
            blend_constant: None,
        }
    }
}

/// Record one full-screen triangle into `target` with `bind_groups` at indices 0, 1, and so on.
pub(crate) fn draw_fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: FullscreenTarget<'_>,
    pipeline: &wgpu::RenderPipeline,
    bind_groups: &[&wgpu::BindGroup],
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: target.load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    if let Some(constant) = target.blend_constant {
        pass.set_blend_constant(constant);
    }
    for (index, group) in bind_groups.iter().enumerate() {
        pass.set_bind_group(index as u32, *group, &[]);
    }
    pass.draw(0..3, 0..1);
}
