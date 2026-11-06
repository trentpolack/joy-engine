// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Immediate triangle rendering with a game-supplied WGSL shader.
//!
//! Arcade games build world-space triangle lists every frame (see [`add_circle`] and friends) and project them with
//! their own shader and uniform block, while the engine owns the GPU resources, depth, and display processing. Each
//! frame draws opaque triangles with depth writes, then caller-sorted transparent triangles blended over them without
//! depth writes, into the HDR scene target; the shared post-processing chain then presents it.
//!
//! Particles draw through the engine's own instanced particle shader rather than the game's ([`TriangleParticles`]):
//! the frame names the camera, the billboard axes, the game's color convention, and where the particles fall among the
//! transparent triangles, and the renderer culls, sorts, and draws them with each emitter's blend mode.
//!
//! [`add_circle`]: crate::add_circle

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::math::{
    Mat4,
    Vec2,
    Vec3,
};

use crate::particles::ParticleFrame;
use crate::pipelines::{
    DEPTH_FORMAT,
    PARTICLE_SHAPES_SOURCE,
    PARTICLE_TRIANGLES_SOURCE,
    ParticlePass,
    ParticleView,
    uniform_entry,
};
use crate::post_process::PostProcessSettings;
use crate::postprocessor::PostProcessor;
use crate::texture_mips::MipmapGenerator;
use crate::{
    ColorVertex,
    DisplayOutput,
    GpuContext,
    PresentationTime,
    RenderError,
};
use joy_core::LinearColor;

/// Post-processing markers authored shaders may contain; they expand so geometry shaders leave display processing
/// to the post chain.
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
    /// one uniform block at `@group(0) @binding(0)`. Authored `JOY_POST_*` markers are accepted: the uniform and
    /// declaration markers expand to nothing and the apply marker to `return sourceColor;`. Fragment output is linear
    /// scene color; display processing happens afterwards. The shader is trusted project code: an invalid shader is a
    /// programming error reported by wgpu, not a recoverable failure.
    pub shader: String,
    /// Size in bytes of the uniform block; a positive multiple of 16.
    pub uniform_size: usize,
    /// Post-processing; `None` presents scene color unchanged ([`PostProcessSettings::NEUTRAL`]).
    pub post_process: Option<PostProcessSettings>,
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
    /// Linear background color.
    pub clear_color: LinearColor,
    /// Instanced particles drawn among the transparent triangles; `None` draws none.
    pub particles: Option<TriangleParticles<'a>>,
}

/// Particles for a [`TriangleFrame`], drawn with the engine's particle shader in the game's camera.
///
/// Batches outside `view_projection`'s view are culled; the rest draw in one block after the first
/// [`after_triangles`](Self::after_triangles) transparent triangles, ordered back to front along `backward` among
/// themselves, with the particles of straight and premultiplied batches ordered the same way inside each batch.
#[derive(Clone, Copy, Debug)]
pub struct TriangleParticles<'a> {
    /// The particles, in world units.
    pub frame: &'a ParticleFrame,
    /// The world-to-clip matrix the game's shader projects with.
    pub view_projection: Mat4,
    /// The unit world axis billboards extend along to their right.
    pub right: Vec3,
    /// The unit world axis billboards extend along upward.
    pub up: Vec3,
    /// The unit world direction toward the viewer: streak trails project onto the plane it faces, and particles order
    /// back to front along it.
    pub backward: Vec3,
    /// How many whole transparent triangles draw before the particles (the caller's layering); the rest draw after.
    /// Values past the end draw the particles last.
    pub after_triangles: usize,
    /// Whether particle RGB holds display (sRGB) values the renderer decodes to linear light, matching a game shader
    /// that decodes its vertex colors.
    pub display_colors: bool,
    /// Multiplier on the particles' linear RGB, such as the game's frame flash.
    pub color_scale: f32,
}

impl<'a> TriangleParticles<'a> {
    /// Particles in the arcade games' XY world plane seen from +Z (billboards span +X and +Y), drawn after every
    /// transparent triangle with linear colors.
    pub fn xy_plane(frame: &'a ParticleFrame, view_projection: Mat4) -> Self {
        Self {
            frame,
            view_projection,
            right: Vec3::X,
            up: Vec3::Y,
            backward: Vec3::Z,
            after_triangles: usize::MAX,
            display_colors: false,
            color_scale: 1.0,
        }
    }
}

/// The particle pass's view block (`ParticleView` in particle-triangles.wgsl). 128 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ParticleViewUniforms {
    view_projection: [[f32; 4]; 4],
    right: [f32; 4],
    up: [f32; 4],
    backward: [f32; 4],
    color: [f32; 4],
}

/// GPU resources for particles, created with the first frame that draws them.
struct TriangleParticleState {
    // The pass owns pipelines that reference the view layout; drop it first.
    pass: ParticlePass,
    mipmaps: MipmapGenerator,
    view_buffer: wgpu::Buffer,
    view_group: wgpu::BindGroup,
    /// Prepared batch indices in drawing order, reused between frames.
    order: Vec<usize>,
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

    // Instanced particles, once a frame draws them.
    particles: Option<TriangleParticleState>,

    // Drawing-buffer size in physical pixels, the logical size games lay out in, and the shell's presentation clock
    // (film grain).
    physical_size: (u32, u32),
    logical_size: Vec2,
    presentation_time: PresentationTime,

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
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let settings = options.post_process.unwrap_or(PostProcessSettings::NEUTRAL);
        let post = PostProcessor::new(device, gpu.queue(), scene_format, output_format, settings);

        Ok(Self {
            opaque_pipeline,
            transparent_pipeline,
            uniform_buffer,
            bind_group,
            uniform_size: options.uniform_size,
            vertex_buffer: None,
            vertex_capacity: 0,
            post,
            particles: None,
            physical_size: (0, 0),
            logical_size: Vec2::ZERO,
            presentation_time: PresentationTime::default(),
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

    /// Replace post-processing settings; cheap enough to call every frame.
    pub fn set_post_process(&mut self, settings: PostProcessSettings) {
        self.post.set_settings(settings);
    }

    /// The presentation target's encoding and HDR headroom.
    pub fn display_output(&self) -> DisplayOutput {
        self.post.display_output()
    }

    /// Write for `display` from the next frame; the shell sets it when the surface is configured and as headroom
    /// changes, so it matches the target views passed to [`TriangleRenderer::encode`].
    pub fn set_display_output(&mut self, display: DisplayOutput) {
        self.post.set_display_output(display);
    }

    /// Set the presentation clock for the next frame; `RenderTarget::draw_triangles` copies the shell's clock here.
    pub fn set_presentation_time(&mut self, time: PresentationTime) {
        self.presentation_time = time;
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
        let size = (width, height);
        self.post.prepare(device, queue, size, size);
        queue.write_buffer(&self.uniform_buffer, 0, frame.uniforms);

        // Particles cull and sort for the game's camera before the pass opens.
        if let Some(particles) = &frame.particles {
            self.prepare_particles(particles)?;
        } else if let Some(state) = &mut self.particles {
            state.order.clear();
        }

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
            let [red, green, blue, alpha] = frame.clear_color.to_array().map(f64::from);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("triangles"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.post.scene_view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: red,
                            g: green,
                            b: blue,
                            a: alpha,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.post.depth_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            // Transparent triangles split around the particles at the caller's layer.
            let opaque_count = opaque.len() as u32;
            let transparent_count = transparent.len() as u32;
            let particle_layer = frame.particles.as_ref().map_or(transparent_count, |particles| {
                (particles.after_triangles.min(transparent.len() / 3) * 3) as u32
            });
            let before = opaque_count..(opaque_count + particle_layer);
            let after = (opaque_count + particle_layer)..(opaque_count + transparent_count);
            if let Some(buffer) = &self.vertex_buffer
                && total > 0
            {
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..total));
                if opaque_count > 0 {
                    pass.set_pipeline(&self.opaque_pipeline);
                    pass.draw(0..opaque_count, 0..1);
                }
                if !before.is_empty() {
                    pass.set_pipeline(&self.transparent_pipeline);
                    pass.draw(before, 0..1);
                }
            }
            if let Some(state) = &self.particles
                && !state.order.is_empty()
            {
                pass.set_bind_group(0, &state.view_group, &[]);
                for index in &state.order {
                    state.pass.draw(&mut pass, *index);
                }
            }
            if let Some(buffer) = &self.vertex_buffer
                && !after.is_empty()
            {
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..total));
                pass.set_pipeline(&self.transparent_pipeline);
                pass.draw(after, 0..1);
            }
        }
        if let Some(state) = &mut self.particles {
            state.pass.release_dropped();
        }
        self.post.encode(encoder, output);
        Ok(())
    }

    /// Record and submit one frame into `output`.
    pub fn render(&mut self, output: &wgpu::TextureView, frame: &TriangleFrame<'_>) -> Result<(), RenderError> {
        let mut encoder = self.gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("triangle frame"),
        });
        self.encode(&mut encoder, output, frame)?;
        self.gpu.queue().submit([encoder.finish()]);
        Ok(())
    }

    /// Cull, order, and upload `particles` for the frame, creating the particle resources on first use.
    fn prepare_particles(&mut self, particles: &TriangleParticles<'_>) -> Result<(), RenderError> {
        let gpu = self.gpu.clone();
        let state = self.particles.get_or_insert_with(|| TriangleParticleState::new(&gpu));
        let uniforms = ParticleViewUniforms {
            view_projection: particles.view_projection.to_cols_array_2d(),
            right: particles.right.extend(0.0).to_array(),
            up: particles.up.extend(0.0).to_array(),
            backward: particles.backward.extend(0.0).to_array(),
            color: [if particles.display_colors { 1.0 } else { 0.0 }, particles.color_scale, 0.0, 0.0],
        };
        gpu.queue().write_buffer(&state.view_buffer, 0, bytemuck::bytes_of(&uniforms));

        // An orthographic arcade camera orders by distance along the view direction alone, so the eye can sit at the
        // origin.
        let view = ParticleView {
            view_projection: particles.view_projection,
            eye: Vec3::ZERO,
            forward: -particles.backward.normalize_or(Vec3::Z),
        };
        state.pass.prepare(&gpu, &mut state.mipmaps, Some(particles.frame), &view)?;

        // Farthest batch first; equal depths keep frame order.
        let batches = state.pass.batches();
        state.order.clear();
        state.order.extend(0..batches.len());
        state.order.sort_by(|left, right| batches[*right].depth.total_cmp(&batches[*left].depth));
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

impl TriangleParticleState {
    /// Compile the particle pipelines for the device's scene format with this renderer's view block.
    fn new(gpu: &GpuContext) -> Self {
        let device = gpu.device();
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("triangle particle view"),
            entries: &[uniform_entry(0)],
        });
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("triangle particle view"),
            size: std::mem::size_of::<ParticleViewUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("triangle particle view"),
            layout: &view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });
        let mut mipmaps = MipmapGenerator::new(device);
        let source = format!("{PARTICLE_SHAPES_SOURCE}\n{PARTICLE_TRIANGLES_SOURCE}");
        let pass = ParticlePass::new(gpu, &mut mipmaps, &[&view_layout], source, gpu.capabilities().scene_format);
        Self {
            pass,
            mipmaps,
            view_buffer,
            view_group,
            order: Vec::new(),
        }
    }
}

/// Expand the `JOY_POST_*` markers for a shader whose display processing happens in the post chain.
pub(crate) fn expand_post_markers(source: &str) -> String {
    source
        .replace(POST_UNIFORMS_MARKER, "")
        .replace(POST_DECLARATIONS_MARKER, "")
        .replace(POST_APPLY_MARKER, "return sourceColor;")
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
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
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
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
#[path = "triangle_renderer.test.rs"]
mod tests;
