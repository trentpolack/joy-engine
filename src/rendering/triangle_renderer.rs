// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Immediate triangle rendering with a game-supplied WGSL shader.
//!
//! Arcade games build world-space triangle lists every frame (see [`add_circle`] and friends) and project them with
//! their own shader and uniform block, while the engine owns the GPU resources, depth, and display processing. Each
//! frame draws opaque triangles with depth writes, then caller-sorted transparent triangles blended over them without
//! depth writes, into the HDR scene target; the shared post-processing chain then presents it.
//!
//! [`add_circle`]: crate::rendering::add_circle

use glam::Vec2;

use crate::core::post_processing::ToneMapper;
use crate::core::{
    PostProcessingConfig,
    Rgba,
};
use crate::rendering::materials::{
    DEPTH_FORMAT,
    uniform_entry,
};
use crate::rendering::postprocessor::PostProcessor;
use crate::rendering::{
    ColorVertex,
    GpuContext,
    RenderError,
};

/// Shader markers the TypeScript engine expands so geometry shaders leave display processing to the post chain.
const POST_UNIFORMS_MARKER: &str = "// JOY_POST_UNIFORMS_MACRO";
const POST_DECLARATIONS_MARKER: &str = "// JOY_POST_DECLARATIONS_MACRO";
const POST_APPLY_MARKER: &str = "// JOY_POST_APPLY_MACRO";

/// Bytes per [`ColorVertex`]: position `vec3f` then color `vec4f`.
const VERTEX_STRIDE: u64 = std::mem::size_of::<ColorVertex>() as u64;

/// Smallest vertex allocation; grows by doubling.
const MINIMUM_VERTEX_CAPACITY: u64 = 4096 * VERTEX_STRIDE;

/// Construction settings; the renderer copies them.
#[derive(Clone, Debug)]
pub struct TriangleRendererOptions {
    /// WGSL source with `vertexMain(@location(0) position: vec3f, @location(1) color: vec4f)` and `fragmentMain`, and
    /// one uniform block at `@group(0) @binding(0)`. The TypeScript `JOY_POST_*` markers are accepted: the uniform and
    /// declaration markers expand to nothing and the apply marker to `return sourceColor;`. Fragment output is linear
    /// scene color; display processing happens afterwards. The shader is trusted project code: an invalid shader is a
    /// programming error reported by wgpu, not a recoverable failure.
    pub shader: String,
    /// Size in bytes of the uniform block; a positive multiple of 16.
    pub uniform_size: usize,
    /// Display processing; `None` presents scene color unchanged (no tone mapping or effects).
    pub post_processing: Option<PostProcessingConfig>,
}

/// Borrowed inputs for one frame. Geometry is world space in the caller's units; the caller's shader projects it.
#[derive(Clone, Copy, Debug)]
pub struct TriangleFrame<'a> {
    /// Opaque triangles (three vertices each), drawn with depth writes.
    pub opaque: &'a [ColorVertex],
    /// Transparent triangles in back-to-front order (see `sort_top_down_transparent_triangles`), blended with
    /// straight alpha and tested against opaque depth without writing it.
    pub transparent: &'a [ColorVertex],
    /// Uniform block bytes; exactly `uniform_size` long.
    pub uniforms: &'a [u8],
    /// Linear RGBA background.
    pub clear_color: Rgba,
    /// Animation time in seconds (film grain).
    pub time_seconds: f32,
}

/// Owns the pipelines, buffers, and post-processing for a game-supplied triangle shader.
pub struct TriangleRenderer {
    // Pipelines and bindings.
    opaque_pipeline: wgpu::RenderPipeline,
    transparent_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    uniform_size: usize,

    // Geometry allocation shared by both phases; grows by doubling and is never shrunk.
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_capacity: u64,

    // Display processing into the presentation format.
    post: PostProcessor,

    // Drawing-buffer size in physical pixels and the logical size games lay out in.
    physical_size: (u32, u32),
    logical_size: Vec2,

    gpu: GpuContext,
}

impl TriangleRenderer {
    /// Compile the shader for the device's scene format and prepare post-processing into `output_format`.
    pub fn new(
        gpu: &GpuContext,
        output_format: wgpu::TextureFormat,
        options: TriangleRendererOptions,
    ) -> Result<Self, RenderError> {
        if options.uniform_size == 0 || !options.uniform_size.is_multiple_of(16) {
            return Err(RenderError::InvalidFrame("uniform size must be a positive multiple of 16 bytes".into()));
        }
        let device = gpu.device();
        let scene_format = gpu.capabilities().scene_format;
        let source = expand_post_markers(&options.shader);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("triangle shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("triangle uniforms"),
            entries: &[uniform_entry(0)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("triangles"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // Opaque triangles write depth; transparent ones test it and blend with straight alpha.
        let opaque_pipeline = triangle_pipeline(device, &module, &pipeline_layout, scene_format, true, None);
        let transparent_pipeline = triangle_pipeline(
            device,
            &module,
            &pipeline_layout,
            scene_format,
            false,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("triangle uniforms"),
            size: options.uniform_size as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("triangle uniforms"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() }],
        });
        let post_config = options.post_processing.unwrap_or_else(neutral_post_processing);
        let post = PostProcessor::new(device, scene_format, output_format, post_config);

        Ok(Self {
            opaque_pipeline,
            transparent_pipeline,
            uniform_buffer,
            bind_group,
            uniform_size: options.uniform_size,
            vertex_buffer: None,
            vertex_capacity: 0,
            post,
            physical_size: (0, 0),
            logical_size: Vec2::ZERO,
            gpu: gpu.clone(),
        })
    }

    /// Resize the drawing buffer. `scale_factor` is physical pixels per logical pixel; the logical size games lay out
    /// in is the physical size divided by it.
    pub fn resize(&mut self, physical_width: u32, physical_height: u32, scale_factor: f32) {
        self.physical_size = (physical_width, physical_height);
        let scale = if scale_factor > 0.0 { scale_factor } else { 1.0 };
        self.logical_size = Vec2::new(physical_width as f32, physical_height as f32) / scale;
    }

    /// Drawing-buffer size in physical pixels.
    pub fn physical_size(&self) -> (u32, u32) {
        self.physical_size
    }

    /// Viewport size in logical pixels.
    pub fn logical_size(&self) -> Vec2 {
        self.logical_size
    }

    /// Replace display settings.
    pub fn set_post_processing(&mut self, config: PostProcessingConfig) {
        self.post.set_config(config);
    }

    /// Record one frame into `encoder`, ending in `output`. Nothing is submitted.
    pub fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        frame: &TriangleFrame<'_>,
    ) -> Result<(), RenderError> {
        let (width, height) = self.physical_size;
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidFrame("the drawing buffer has no area; call resize first".into()));
        }
        if frame.uniforms.len() != self.uniform_size {
            return Err(RenderError::InvalidFrame(format!(
                "uniform data is {} bytes; the shader's block is {} bytes",
                frame.uniforms.len(),
                self.uniform_size
            )));
        }
        let gpu = self.gpu.clone();
        let (device, queue) = (gpu.device(), gpu.queue());
        self.post.prepare(device, queue, width, height, frame.time_seconds);
        queue.write_buffer(&self.uniform_buffer, 0, frame.uniforms);

        // One allocation holds opaque then transparent vertices; incomplete trailing triangles are dropped.
        let opaque = &frame.opaque[..frame.opaque.len() - (frame.opaque.len() % 3)];
        let transparent = &frame.transparent[..frame.transparent.len() - (frame.transparent.len() % 3)];
        let total = (opaque.len() + transparent.len()) as u64 * VERTEX_STRIDE;
        if total > 0 {
            self.ensure_vertex_capacity(total);
            let buffer = self.vertex_buffer.as_ref().expect("allocated above");
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(opaque));
            queue.write_buffer(buffer, opaque.len() as u64 * VERTEX_STRIDE, bytemuck::cast_slice(transparent));
        }

        {
            let [red, green, blue, alpha] = frame.clear_color.map(f64::from);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("triangles"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.post.scene_view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: red, g: green, b: blue, a: alpha }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.post.depth_view(),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            if let Some(buffer) = &self.vertex_buffer
                && total > 0
            {
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..total));
                let opaque_count = opaque.len() as u32;
                let transparent_count = transparent.len() as u32;
                if opaque_count > 0 {
                    pass.set_pipeline(&self.opaque_pipeline);
                    pass.draw(0..opaque_count, 0..1);
                }
                if transparent_count > 0 {
                    pass.set_pipeline(&self.transparent_pipeline);
                    pass.draw(opaque_count..(opaque_count + transparent_count), 0..1);
                }
            }
        }
        self.post.encode(encoder, output);
        Ok(())
    }

    /// Record and submit one frame into `output`.
    pub fn render(&mut self, output: &wgpu::TextureView, frame: &TriangleFrame<'_>) -> Result<(), RenderError> {
        let mut encoder =
            self.gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("triangle frame") });
        self.encode(&mut encoder, output, frame)?;
        self.gpu.queue().submit([encoder.finish()]);
        Ok(())
    }

    /// Grow the shared vertex allocation to at least `bytes`.
    fn ensure_vertex_capacity(&mut self, bytes: u64) {
        if self.vertex_buffer.is_some() && self.vertex_capacity >= bytes {
            return;
        }
        let capacity = bytes.next_power_of_two().max(MINIMUM_VERTEX_CAPACITY);
        self.vertex_buffer = Some(self.gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("triangle vertices"),
            size: capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.vertex_capacity = capacity;
    }
}

/// Expand the TypeScript post-processing markers for a shader whose display processing happens in the post chain.
pub(crate) fn expand_post_markers(source: &str) -> String {
    source
        .replace(POST_UNIFORMS_MARKER, "")
        .replace(POST_DECLARATIONS_MARKER, "")
        .replace(POST_APPLY_MARKER, "return sourceColor;")
}

/// Settings that present scene color unchanged.
fn neutral_post_processing() -> PostProcessingConfig {
    PostProcessingConfig {
        tone_mapper: ToneMapper::None,
        bloom_strength: 0.0,
        blur_radius: 0.0,
        vignette_blur: 0.0,
        vignette_strength: 0.0,
        film_grain: 0.0,
        antialias_strength: 0.0,
        ..PostProcessingConfig::default()
    }
}

/// One of the two triangle pipelines: opaque (depth writes) or transparent (blended, depth tested only).
fn triangle_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    write_depth: bool,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if write_depth { "opaque triangles" } else { "transparent triangles" }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_STRIDE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(write_depth),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_markers_expand_like_the_typescript_engine() {
        let source = "struct U {\n  a: vec4f,\n  // JOY_POST_UNIFORMS_MACRO\n};\n// JOY_POST_DECLARATIONS_MACRO\nfn f() -> vec4f {\n  let sourceColor = vec4f(1.0);\n  // JOY_POST_APPLY_MACRO\n}";
        let expanded = expand_post_markers(source);
        assert!(!expanded.contains("JOY_POST"));
        assert!(expanded.contains("return sourceColor;"));
        assert_eq!(expand_post_markers("plain"), "plain");
    }
}
