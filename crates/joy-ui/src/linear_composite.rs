// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! UI over extended linear (scRGB) presentation targets.
//!
//! egui's painter writes gamma-space colors unless its target format encodes sRGB itself, so it cannot paint straight
//! into a linear float surface. On those surfaces egui paints into an sRGB-format intermediate instead (its linear
//! framebuffer path), and [`LinearComposite`] blends that intermediate over the scene, scaled so UI white matches the
//! display's SDR white. Standard and extended sRGB targets do not need it.

/// Format egui paints into before compositing: sRGB encoding keeps 8-bit UI gradients smooth and decodes on load.
pub(crate) const INTERFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The composite pass.
const COMPOSITE_SOURCE: &str = include_str!("shaders/ui-composite.wgsl");

/// Byte size of `Composite` in `ui-composite.wgsl`.
const UNIFORM_SIZE: u64 = 16;

/// Owns the UI intermediate and the pass that blends it over a linear presentation target.
pub(crate) struct LinearComposite {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    /// The intermediate at the current presentation size, created on first use and on resize.
    target: Option<InterfaceTarget>,
}

/// The intermediate texture egui paints into, and the composite's bindings for it.
struct InterfaceTarget {
    size: [u32; 2],
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl LinearComposite {
    /// Create the composite pass for presentation views of `output_format`, with SDR white at `sdr_white_level`.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output_format: wgpu::TextureFormat,
        sdr_white_level: f32,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui composite"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui composite"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SOURCE.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertexMain"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragmentMain"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui composite"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite = Self { pipeline, layout, uniforms, target: None };
        composite.set_sdr_white_level(queue, sdr_white_level);
        composite
    }

    /// Scale UI light so its white lands on `sdr_white_level` from the next composite.
    pub(crate) fn set_sdr_white_level(&self, queue: &wgpu::Queue, sdr_white_level: f32) {
        let values = [sdr_white_level, 0.0, 0.0, 0.0];
        let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_ne_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
    }

    /// The intermediate for a `size` (pixels) presentation target, cleared by the caller's first pass each frame.
    pub(crate) fn interface_view(&mut self, device: &wgpu::Device, size: [u32; 2]) -> &wgpu::TextureView {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_none_or(|target| target.size != size) {
            self.target = Some(self.create_target(device, size));
        }
        &self.target.as_ref().expect("the interface target exists after creation").view
    }

    /// Blend the intermediate over `output`.
    pub(crate) fn composite(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let Some(target) = &self.target else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &target.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Allocate the intermediate and bind it.
    fn create_target(&self, device: &wgpu::Device, size: [u32; 2]) -> InterfaceTarget {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui intermediate"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: INTERFACE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui composite"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            ],
        });
        InterfaceTarget { size, view, bind_group }
    }
}

#[cfg(test)]
#[path = "linear_composite.test.rs"]
mod tests;
