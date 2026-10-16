// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Auxiliary scene attachments for effects:
//! view-space normals with roughness (drawn against the scene depth with an equal test) and screen-space motion vectors
//! from each instance's current and previous model matrices.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::rendering::materials::{DEPTH_FORMAT, INSTANCE_ATTRIBUTES, InstanceRaw, surface_pipeline};
use crate::rendering::mesh::MeshVertex;

/// Normal and roughness attachment format.
const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Motion attachment format (signed screen-space UV offsets).
const MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// An owned color texture and its view.
pub(crate) struct Attachment {
    pub(crate) view: wgpu::TextureView,
    _texture: wgpu::Texture,
    size: (u32, u32),
}

impl Attachment {
    fn new(device: &wgpu::Device, label: &str, size: (u32, u32), format: wgpu::TextureFormat) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self { view: texture.create_view(&wgpu::TextureViewDescriptor::default()), _texture: texture, size }
    }
}

/// The normal and roughness pass.
pub(crate) struct SurfacePass {
    pipeline: wgpu::RenderPipeline,
    target: Option<Attachment>,
}

impl SurfacePass {
    pub(crate) fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene surfaces"),
            bind_group_layouts: &[Some(frame_layout)],
            immediate_size: 0,
        });
        let depth = wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Equal),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let pipeline = surface_pipeline(
            device,
            "scene surfaces",
            include_str!("../shaders/scene-surfaces.wgsl"),
            &layout,
            SURFACE_FORMAT,
            None,
            depth,
        );
        Self { pipeline, target: None }
    }

    /// Ensure the attachment matches `size`, then open a pass over it reading `depth`; the caller draws geometry.
    pub(crate) fn begin<'a>(
        &'a mut self,
        device: &wgpu::Device,
        encoder: &'a mut wgpu::CommandEncoder,
        depth: &'a wgpu::TextureView,
        size: (u32, u32),
    ) -> wgpu::RenderPass<'a> {
        if self.target.as_ref().map(|target| target.size) != Some(size) {
            self.target = Some(Attachment::new(device, "scene normal roughness", size, SURFACE_FORMAT));
        }
        let target = self.target.as_ref().map(|target| &target.view).expect("allocated above");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene surfaces"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                // Uncovered pixels face the camera.
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.5, g: 0.5, b: 1.0, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: None,
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass
    }

    /// The attachment written by the latest pass.
    pub(crate) fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|target| &target.view)
    }
}

/// Motion pass uniforms (`Uniforms` in scene-motion.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct MotionUniforms {
    pub(crate) current_view_projection: [[f32; 4]; 4],
    pub(crate) previous_view_projection: [[f32; 4]; 4],
    pub(crate) raster_view_projection: [[f32; 4]; 4],
    pub(crate) velocity_jitter_delta: [f32; 4],
}

/// Unjittered and jittered motion targets with their own depth.
struct MotionTargets {
    unjittered: Attachment,
    jittered: Attachment,
    depth_view: wgpu::TextureView,
    _depth: wgpu::Texture,
    size: (u32, u32),
}

/// The motion-vector pass.
pub(crate) struct MotionPass {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    previous_models: wgpu::Buffer,
    previous_capacity: usize,
    targets: Option<MotionTargets>,
}

/// Previous model matrix columns, locations 11 through 14.
const PREVIOUS_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![11 => Float32x4, 12 => Float32x4, 13 => Float32x4, 14 => Float32x4];

impl MotionPass {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene motion"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene motion uniforms"),
            size: std::mem::size_of::<MotionUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene motion"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene motion"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/scene-motion.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene motion"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let mesh_attributes =
            [wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<MeshVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &mesh_attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<InstanceRaw>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES[..4],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &PREVIOUS_ATTRIBUTES,
            }),
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene motion"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertexMain"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragmentMain"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: MOTION_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: MOTION_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let previous_models = previous_buffer(device, 64);
        Self { pipeline, uniforms, bind_group, previous_models, previous_capacity: 64, targets: None }
    }

    /// Upload uniforms and previous models (parallel to the instance buffer), then open a pass; the caller draws
    /// geometry with the instance buffer in slot 1. Slot 2 is bound here.
    pub(crate) fn begin<'a>(
        &'a mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &'a mut wgpu::CommandEncoder,
        uniforms: &MotionUniforms,
        previous_models: &[Mat4],
        size: (u32, u32),
    ) -> wgpu::RenderPass<'a> {
        if self.targets.as_ref().map(|targets| targets.size) != Some(size) {
            let depth = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene motion depth"),
                size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.targets = Some(MotionTargets {
                unjittered: Attachment::new(device, "scene motion unjittered", size, MOTION_FORMAT),
                jittered: Attachment::new(device, "scene motion jittered", size, MOTION_FORMAT),
                depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
                _depth: depth,
                size,
            });
        }
        if previous_models.len() > self.previous_capacity {
            self.previous_capacity = previous_models.len().next_power_of_two();
            self.previous_models = previous_buffer(device, self.previous_capacity);
        }
        let columns: Vec<[[f32; 4]; 4]> = previous_models.iter().map(Mat4::to_cols_array_2d).collect();
        queue.write_buffer(&self.previous_models, 0, bytemuck::cast_slice(&columns));
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));

        let targets = self.targets.as_ref().expect("allocated above");
        let clear =
            wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene motion"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &targets.unjittered.view,
                    resolve_target: None,
                    ops: clear,
                    depth_slice: None,
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &targets.jittered.view,
                    resolve_target: None,
                    ops: clear,
                    depth_slice: None,
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(2, self.previous_models.slice(..));
        pass
    }

    /// Unjittered motion (antialiasing and motion blur).
    pub(crate) fn unjittered(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|targets| &targets.unjittered.view)
    }

    /// Jittered motion (lighting effects).
    pub(crate) fn jittered(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|targets| &targets.jittered.view)
    }
}

fn previous_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene motion previous models"),
        size: (capacity * 64) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
