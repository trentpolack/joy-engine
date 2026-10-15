// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame orchestrator: owns pipelines, shared targets, GPU mesh uploads, and pass order.
//!
//! One frame proceeds as: validate inputs, build camera matrices, cull and batch instances, upload instance and frame
//! data, render the directional shadow map, render the scene (sky, opaque batches, transient opaque triangles, then
//! sorted transparency) into the HDR target, and run post-processing into the caller's presentation view. Materials
//! define shading; the renderer defines frame-wide invariants and submission order.

use std::collections::HashMap;
use std::sync::Weak;

use glam::{Mat4, Vec2, Vec3};
use joy_foundation::post_processing::PostProcessingConfig;
use wgpu::util::DeviceExt;

use crate::RenderError;
use crate::camera::{Camera, CameraFrame, WorldRay, intersect_ray_plane, project_to_viewport, screen_ray};
use crate::frame::{ColorVertex, FrameInputs, MAX_POINT_LIGHTS, sort_transparent_triangles};
use crate::gpu::GpuContext;
use crate::mesh::{Bounds, MeshAsset, MeshVertex, Submesh, VertexMaterial};
use crate::pipelines::{
    AtmosphereUniforms, CasterBias, CasterUniforms, FrameUniforms, GpuPointLight, InstanceRaw, SHADOW_FORMAT,
    ScenePipelines,
};
use crate::post::PostProcessor;
use crate::scene::{MeshInstance, RenderScene, Shading};
use crate::shadows::{ShadowOptions, fit_directional_shadow};

/// Initial instance buffer capacity; grows geometrically.
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Initial transient vertex capacity; grows geometrically.
const INITIAL_TRANSIENT_CAPACITY: usize = 1024;

/// Renderer construction settings. The renderer copies them.
#[derive(Clone, Debug, PartialEq)]
pub struct RendererOptions {
    /// Directional shadows from the environment's primary light; `None` disables shadow work.
    pub shadows: Option<ShadowOptions>,
    /// Draw the procedural sky when an environment is supplied.
    pub atmosphere: bool,
    /// Display settings; replace later with [`Renderer::set_post_processing`].
    pub post_processing: PostProcessingConfig,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self {
            shadows: Some(ShadowOptions::default()),
            atmosphere: true,
            post_processing: PostProcessingConfig::default(),
        }
    }
}

/// Copied per-frame statistics for development overlays. No GPU queries are issued to produce them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererDiagnostics {
    /// Graphics API label.
    pub backend: &'static str,
    /// Adapter name.
    pub adapter: String,
    /// Whether the scene target preserves HDR highlights.
    pub hdr: bool,
    /// Instances drawn by the camera.
    pub visible: usize,
    /// Instances skipped by frustum culling.
    pub culled: usize,
    /// Draw calls recorded for the scene and shadow passes.
    pub draw_calls: usize,
    /// Triangles submitted to the scene pass.
    pub triangles: usize,
    /// Instances rendered into the shadow map.
    pub shadow_casters: usize,
    /// Whether the directional shadow map was rendered this frame.
    pub shadows_active: bool,
}

/// Which surface pipeline draws an opaque batch; ordering is draw order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum OpaquePipeline {
    Lit,
    Pbr,
    Unlit,
}

/// Which pipeline draws a blended range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlendPipeline {
    Unlit,
    Pbr,
}

/// One opaque instance waiting to be batched.
struct OpaqueItem {
    pipeline: OpaquePipeline,
    asset: MeshAsset,
    raw: InstanceRaw,
}

/// One blended instance range waiting to be sorted.
struct BlendItem {
    distance: f32,
    pipeline: BlendPipeline,
    asset: MeshAsset,
    indices: std::ops::Range<u32>,
    raw: InstanceRaw,
}

/// A recorded batch: one asset submesh drawn for a contiguous instance range.
/// Shadow-caster batches share this record; the caster pass ignores `pipeline`.
struct Batch {
    pipeline: OpaquePipeline,
    asset_id: u64,
    indices: std::ops::Range<u32>,
    instances: std::ops::Range<u32>,
}

/// GPU copy of one asset, released when the asset is dropped everywhere.
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    asset: Weak<dyn std::any::Any + Send + Sync>,
}

/// Owned directional shadow map resources.
struct ShadowMap {
    // Bind groups and views reference the texture; drop them first.
    receiver_bind_group: wgpu::BindGroup,
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    size: u32,
}

/// The engine's renderer. Borrows scenes and cameras per frame; owns all GPU resources it creates.
pub struct Renderer {
    // Pipelines and per-frame buffers.
    pipelines: ScenePipelines,
    post: PostProcessor,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    caster_buffer: wgpu::Buffer,
    caster_bind_group: wgpu::BindGroup,
    atmosphere_buffer: wgpu::Buffer,
    atmosphere_bind_group: wgpu::BindGroup,
    shadow_map: Option<ShadowMap>,
    unshadowed_bind_group: wgpu::BindGroup,
    _unshadowed_view: wgpu::TextureView,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    transient_buffer: wgpu::Buffer,
    transient_capacity: usize,

    // Uploaded mesh copies keyed by asset identity.
    meshes: HashMap<u64, GpuMesh>,

    // Configuration and drawing size.
    options: RendererOptions,
    physical_size: (u32, u32),
    scale_factor: f32,
    diagnostics: RendererDiagnostics,

    // The shared device; resources above keep it alive internally, so field order is not load-bearing here.
    gpu: GpuContext,
}

impl Renderer {
    /// Create a renderer that presents into views of `output_format`.
    ///
    /// The final pass writes display-referred values without sRGB encoding, matching the TypeScript engine's canvas
    /// path; pass a non-sRGB format (or a non-sRGB view format of an sRGB surface) to reproduce its look.
    pub fn new(gpu: &GpuContext, output_format: wgpu::TextureFormat, options: RendererOptions) -> Self {
        let device = gpu.device();
        let capabilities = gpu.capabilities();
        let bias = options.shadows.map_or(CasterBias { constant: 0, slope_scale: 0.0 }, |shadows| CasterBias {
            constant: shadows.depth_bias,
            slope_scale: shadows.depth_bias_slope_scale,
        });
        let pipelines = ScenePipelines::new(device, capabilities.scene_format, bias);
        let post =
            PostProcessor::new(device, capabilities.scene_format, output_format, options.post_processing.clone());

        let frame_buffer = uniform_buffer(device, "frame uniforms", std::mem::size_of::<FrameUniforms>());
        let frame_bind_group = single_buffer_group(device, "frame", &pipelines.frame_layout, &frame_buffer);
        let caster_buffer = uniform_buffer(device, "caster uniforms", std::mem::size_of::<CasterUniforms>());
        let caster_bind_group = single_buffer_group(device, "caster", &pipelines.caster_layout, &caster_buffer);
        let atmosphere_buffer =
            uniform_buffer(device, "atmosphere uniforms", std::mem::size_of::<AtmosphereUniforms>());
        let atmosphere_bind_group =
            single_buffer_group(device, "atmosphere", &pipelines.atmosphere_layout, &atmosphere_buffer);

        // Comparison sampler shared by the real and placeholder shadow maps.
        let comparison_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow comparison"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        // A 1x1 placeholder keeps the receiver bind group valid when shadows are disabled or inactive.
        let (unshadowed_view, unshadowed_bind_group) = {
            let texture = depth_texture(device, "shadow placeholder", 1);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let group = receiver_group(device, &pipelines.shadow_layout, &view, &comparison_sampler);
            (view, group)
        };
        let shadow_map = options.shadows.map(|shadows| {
            let size = shadows.map_size(capabilities.max_texture_dimension);
            let texture = depth_texture(device, "directional shadow map", size);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let receiver_bind_group = receiver_group(device, &pipelines.shadow_layout, &view, &comparison_sampler);
            ShadowMap { receiver_bind_group, view, _texture: texture, size }
        });

        let instance_buffer =
            vertex_buffer(device, "instances", INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<InstanceRaw>());
        let transient_buffer = vertex_buffer(
            device,
            "transient triangles",
            INITIAL_TRANSIENT_CAPACITY * std::mem::size_of::<MeshVertex>(),
        );

        let diagnostics = RendererDiagnostics {
            backend: capabilities.backend.label(),
            adapter: capabilities.adapter_name.clone(),
            hdr: capabilities.hdr,
            ..Default::default()
        };

        Self {
            pipelines,
            post,
            frame_buffer,
            frame_bind_group,
            caster_buffer,
            caster_bind_group,
            atmosphere_buffer,
            atmosphere_bind_group,
            shadow_map,
            unshadowed_bind_group,
            _unshadowed_view: unshadowed_view,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            transient_buffer,
            transient_capacity: INITIAL_TRANSIENT_CAPACITY,
            meshes: HashMap::new(),
            options,
            physical_size: (0, 0),
            scale_factor: 1.0,
            diagnostics,
            gpu: gpu.clone(),
        }
    }

    /// Set the drawing-buffer size in physical pixels and the display scale factor.
    ///
    /// The logical (CSS) size is the physical size divided by the scale factor; only resolution depends on density,
    /// never the view, so a high-DPI display shows the same scene extent.
    pub fn resize(&mut self, physical_width: u32, physical_height: u32, scale_factor: f32) {
        self.physical_size = (physical_width, physical_height);
        self.scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 { scale_factor } else { 1.0 };
    }

    /// Drawing-buffer size in physical pixels.
    pub fn physical_size(&self) -> (u32, u32) {
        self.physical_size
    }

    /// Viewport size in logical (CSS) pixels.
    pub fn logical_size(&self) -> Vec2 {
        Vec2::new(self.physical_size.0 as f32, self.physical_size.1 as f32) / self.scale_factor
    }

    /// Current display settings.
    pub fn post_processing(&self) -> &PostProcessingConfig {
        self.post.config()
    }

    /// Replace display settings for the next frame; only graph-shape changes reallocate targets.
    pub fn set_post_processing(&mut self, config: PostProcessingConfig) {
        self.options.post_processing = config.clone();
        self.post.set_config(config);
    }

    /// Statistics from the most recent frame.
    pub fn diagnostics(&self) -> &RendererDiagnostics {
        &self.diagnostics
    }

    /// The device this renderer draws with.
    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// Camera matrices for the current logical aspect ratio.
    pub fn camera_frame(&self, camera: &impl Camera) -> CameraFrame {
        let size = self.logical_size();
        camera.camera_frame(if size.y > 0.0 { size.x / size.y } else { 1.0 })
    }

    /// World ray through a logical viewport point (top-left origin); `None` before the first resize.
    pub fn screen_ray(&self, camera: &impl Camera, point: Vec2) -> Option<WorldRay> {
        screen_ray(point, self.camera_frame(camera).view_projection(), self.logical_size())
    }

    /// Intersect a viewport point's ray with the horizontal plane at `height` world units.
    pub fn pick_ground(&self, camera: &impl Camera, point: Vec2, height: f32) -> Option<Vec3> {
        intersect_ray_plane(self.screen_ray(camera, point)?, Vec3::Y, -height)
    }

    /// Project a world position to logical viewport pixels, or `None` behind the camera.
    pub fn project(&self, camera: &impl Camera, position: Vec3) -> Option<Vec2> {
        project_to_viewport(position, self.camera_frame(camera).view_projection(), self.logical_size())
    }

    /// Render and submit one frame into `output`.
    pub fn render(
        &mut self,
        scene: &RenderScene,
        camera: &impl Camera,
        inputs: &FrameInputs<'_>,
        output: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("joy-render frame") });
        self.encode(&mut encoder, scene, camera, inputs, output)?;
        self.gpu.queue().submit([encoder.finish()]);
        Ok(())
    }

    /// Record one frame into `encoder` without submitting, so a host can append overlays (such as development UI).
    ///
    /// Uploads happen immediately through the queue. Fails without recording anything for a zero-sized drawing buffer
    /// or invalid lights.
    pub fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &RenderScene,
        camera: &impl Camera,
        inputs: &FrameInputs<'_>,
        output: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        let (width, height) = self.physical_size;
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidFrame("the drawing buffer has no area; call resize first".into()));
        }
        validate_lights(inputs)?;

        // Camera and culling.
        let camera_frame = self.camera_frame(camera);
        let view_projection = camera_frame.view_projection();
        let mut visible = 0;
        let mut culled = 0;
        let mut opaque_items = Vec::new();
        let mut blend_items = Vec::new();
        let mut caster_items = Vec::new();
        let mut caster_bounds: Option<Bounds> = None;

        for (_, instance) in scene.iter() {
            if !instance.visible || instance.opacity <= 0.0 {
                continue;
            }
            let model = instance.transform.matrix();
            let raw = instance_raw(instance, model);
            let transparent = instance.is_transparent();

            // Shadow casters include camera-culled opaque geometry so off-screen objects still cast into view.
            if instance.casts_shadow && !transparent {
                caster_items.push(OpaqueItem { pipeline: OpaquePipeline::Lit, asset: instance.asset.clone(), raw });
                let world = world_bounds(instance.asset.bounds(), model);
                caster_bounds = Some(caster_bounds.map_or(world, |bounds| union(bounds, world)));
            }

            if outside_frustum(instance.asset.bounds(), model, view_projection) {
                culled += 1;
                continue;
            }
            visible += 1;
            self.queue_instance(
                instance,
                model,
                raw,
                transparent,
                camera_frame.eye,
                &mut opaque_items,
                &mut blend_items,
            );
        }

        // Transient triangles: opaque first, then transparency sorted back to front.
        let backward = (camera_frame.eye - camera_frame.focus).try_normalize().unwrap_or(Vec3::Z);
        let transient = transient_vertices(
            inputs.opaque_triangles,
            &sort_transparent_triangles(inputs.transparent_triangles, backward),
        );
        let transient_opaque_count = (inputs.opaque_triangles.len() / 3 * 3) as u32;

        // Ensure every referenced asset has a GPU copy before passes open.
        for item in opaque_items.iter().chain(&caster_items) {
            self.upload_mesh(&item.asset);
        }
        for item in &blend_items {
            self.upload_mesh(&item.asset);
        }

        // Instance buffer: identity, camera opaque batches, sorted blended ranges, then shadow casters.
        opaque_items.sort_by_key(|item| (item.pipeline, item.asset.id()));
        caster_items.sort_by_key(|item| item.asset.id());
        blend_items.sort_by(|left, right| right.distance.total_cmp(&left.distance));
        let mut raws = vec![InstanceRaw::IDENTITY];
        let opaque_batches = self.batch(&opaque_items, &mut raws);
        let blend_start = raws.len() as u32;
        raws.extend(blend_items.iter().map(|item| item.raw));
        let caster_batches = self.batch(&caster_items, &mut raws);
        self.write_instances(&raws);
        self.write_transient(&transient);

        // Shadow volume, frame uniforms, and sky uniforms.
        let shadow_matrix = self.shadow_matrix(inputs, &camera_frame, caster_bounds, !caster_items.is_empty());
        self.write_frame_uniforms(inputs, &camera_frame, shadow_matrix);
        let draw_sky = self.options.atmosphere && inputs.environment.is_some();
        if draw_sky {
            self.write_atmosphere_uniforms(inputs, &camera_frame);
        }

        // Shadow pass.
        let mut draw_calls = 0;
        if let (Some(_), Some(shadow_map)) = (shadow_matrix, &self.shadow_map) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("directional shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &shadow_map.view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipelines.caster);
            pass.set_bind_group(0, &self.caster_bind_group, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            draw_calls += self.draw_batches(&mut pass, &caster_batches);
            if transient_opaque_count > 0 {
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(0..transient_opaque_count, 0..1);
                draw_calls += 1;
            }
        }

        // Scene pass into the HDR target.
        self.post.prepare(self.gpu.device(), self.gpu.queue(), width, height, inputs.time_seconds);
        let triangles = {
            let clear = inputs.clear_color;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.post.scene_view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(clear[0]),
                            g: f64::from(clear[1]),
                            b: f64::from(clear[2]),
                            a: f64::from(clear[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.post.depth_view(),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            if draw_sky {
                pass.set_pipeline(&self.pipelines.atmosphere);
                pass.set_bind_group(0, &self.atmosphere_bind_group, &[]);
                pass.draw(0..3, 0..1);
                draw_calls += 1;
            }

            let receiver = match (&self.shadow_map, shadow_matrix) {
                (Some(shadow_map), Some(_)) => &shadow_map.receiver_bind_group,
                _ => &self.unshadowed_bind_group,
            };
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, receiver, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));

            // Opaque batches, grouped by pipeline.
            let mut triangles = 0;
            let mut bound = None;
            for batch in &opaque_batches {
                if bound != Some(batch.pipeline) {
                    pass.set_pipeline(self.opaque_pipeline(batch.pipeline));
                    bound = Some(batch.pipeline);
                }
                triangles += self.draw_batch(&mut pass, batch);
                draw_calls += 1;
            }

            // Transient opaque triangles are lit like Lit meshes.
            if transient_opaque_count > 0 {
                pass.set_pipeline(&self.pipelines.lit);
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(0..transient_opaque_count, 0..1);
                draw_calls += 1;
                triangles += (transient_opaque_count / 3) as usize;
            }

            // Sorted blended ranges, then sorted transient transparency.
            for (offset, item) in blend_items.iter().enumerate() {
                let Some(mesh) = self.meshes.get(&item.asset.id()) else {
                    continue;
                };
                pass.set_pipeline(match item.pipeline {
                    BlendPipeline::Unlit => &self.pipelines.unlit_blend,
                    BlendPipeline::Pbr => &self.pipelines.pbr_blend,
                });
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                let instance = blend_start + offset as u32;
                pass.draw_indexed(item.indices.clone(), 0, instance..instance + 1);
                draw_calls += 1;
                triangles += item.indices.len() / 3;
            }
            let transient_total = transient.len() as u32;
            if transient_total > transient_opaque_count {
                pass.set_pipeline(&self.pipelines.unlit_blend);
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(transient_opaque_count..transient_total, 0..1);
                draw_calls += 1;
                triangles += ((transient_total - transient_opaque_count) / 3) as usize;
            }
            triangles
        };

        self.post.encode(encoder, output);
        self.release_dropped_meshes();

        self.diagnostics.visible = visible;
        self.diagnostics.culled = culled;
        self.diagnostics.draw_calls = draw_calls;
        self.diagnostics.triangles = triangles;
        self.diagnostics.shadow_casters = if shadow_matrix.is_some() { caster_items.len() } else { 0 };
        self.diagnostics.shadows_active = shadow_matrix.is_some();
        Ok(())
    }

    /// Route one camera-visible instance into the opaque batches and/or the sorted blend queue.
    #[allow(clippy::too_many_arguments)]
    fn queue_instance(
        &self,
        instance: &MeshInstance,
        model: Mat4,
        raw: InstanceRaw,
        transparent: bool,
        eye: Vec3,
        opaque_items: &mut Vec<OpaqueItem>,
        blend_items: &mut Vec<BlendItem>,
    ) {
        let distance = world_bounds(instance.asset.bounds(), model).center().distance(eye);
        if transparent {
            // Transparent instances are unlit, matching the TypeScript scene path; PBR keeps its own blending.
            let pipeline = if instance.shading == Shading::Pbr { BlendPipeline::Pbr } else { BlendPipeline::Unlit };
            for submesh in instance.asset.submeshes() {
                blend_items.push(BlendItem {
                    distance,
                    pipeline,
                    asset: instance.asset.clone(),
                    indices: submesh.indices.clone(),
                    raw,
                });
            }
            return;
        }

        let pipeline = match instance.shading {
            Shading::Lit => OpaquePipeline::Lit,
            Shading::Pbr => OpaquePipeline::Pbr,
            Shading::Unlit => OpaquePipeline::Unlit,
        };
        opaque_items.push(OpaqueItem { pipeline, asset: instance.asset.clone(), raw });

        // PBR materials can blend part of an otherwise opaque asset.
        if instance.shading == Shading::Pbr {
            for submesh in instance.asset.submeshes().iter().filter(|submesh| submesh.blend) {
                blend_items.push(BlendItem {
                    distance,
                    pipeline: BlendPipeline::Pbr,
                    asset: instance.asset.clone(),
                    indices: submesh.indices.clone(),
                    raw,
                });
            }
        }
    }

    /// Append sorted items to the instance list and group runs of the same pipeline and asset into batches.
    /// Each batch draws the asset's opaque submesh (or all of it when the asset has no opaque range).
    fn batch(&self, items: &[OpaqueItem], raws: &mut Vec<InstanceRaw>) -> Vec<Batch> {
        let mut batches: Vec<Batch> = Vec::new();
        for item in items {
            let instance = raws.len() as u32;
            raws.push(item.raw);
            let pipeline = item.pipeline;
            if let Some(last) = batches.last_mut()
                && last.pipeline == pipeline
                && last.asset_id == item.asset.id()
            {
                last.instances.end = instance + 1;
                continue;
            }
            let indices = opaque_range(item.asset.submeshes());
            batches.push(Batch { pipeline, asset_id: item.asset.id(), indices, instances: instance..instance + 1 });
        }
        batches
    }

    /// Draw caster batches; returns the number of draw calls.
    fn draw_batches(&self, pass: &mut wgpu::RenderPass<'_>, batches: &[Batch]) -> usize {
        for batch in batches {
            self.draw_batch(pass, batch);
        }
        batches.len()
    }

    /// Draw one batch; returns the triangle count.
    fn draw_batch(&self, pass: &mut wgpu::RenderPass<'_>, batch: &Batch) -> usize {
        let Some(mesh) = self.meshes.get(&batch.asset_id) else {
            return 0;
        };
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(batch.indices.clone(), 0, batch.instances.clone());
        (batch.indices.len() / 3) * batch.instances.len()
    }

    fn opaque_pipeline(&self, pipeline: OpaquePipeline) -> &wgpu::RenderPipeline {
        match pipeline {
            OpaquePipeline::Lit => &self.pipelines.lit,
            OpaquePipeline::Pbr => &self.pipelines.pbr,
            OpaquePipeline::Unlit => &self.pipelines.unlit,
        }
    }

    /// Upload an asset the first time it is drawn.
    fn upload_mesh(&mut self, asset: &MeshAsset) {
        if self.meshes.contains_key(&asset.id()) {
            return;
        }
        let device = self.gpu.device();
        let data = asset.data();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&data.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&data.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.meshes.insert(asset.id(), GpuMesh { vertices, indices, asset: asset.downgrade() });
    }

    /// Release uploads whose assets no longer exist anywhere.
    fn release_dropped_meshes(&mut self) {
        self.meshes.retain(|_, mesh| mesh.asset.strong_count() > 0);
    }

    /// Upload instance data, growing the buffer geometrically.
    fn write_instances(&mut self, raws: &[InstanceRaw]) {
        if raws.len() > self.instance_capacity {
            self.instance_capacity = raws.len().next_power_of_two();
            self.instance_buffer = vertex_buffer(
                self.gpu.device(),
                "instances",
                self.instance_capacity * std::mem::size_of::<InstanceRaw>(),
            );
        }
        self.gpu.queue().write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(raws));
    }

    /// Upload transient vertices, growing the buffer geometrically.
    fn write_transient(&mut self, vertices: &[MeshVertex]) {
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > self.transient_capacity {
            self.transient_capacity = vertices.len().next_power_of_two();
            self.transient_buffer = vertex_buffer(
                self.gpu.device(),
                "transient triangles",
                self.transient_capacity * std::mem::size_of::<MeshVertex>(),
            );
        }
        self.gpu.queue().write_buffer(&self.transient_buffer, 0, bytemuck::cast_slice(vertices));
    }

    /// Fit and upload the light camera when the primary light should cast shadows this frame.
    fn shadow_matrix(
        &self,
        inputs: &FrameInputs<'_>,
        camera: &CameraFrame,
        caster_bounds: Option<Bounds>,
        has_casters: bool,
    ) -> Option<Mat4> {
        let options = self.options.shadows?;
        let shadow_map = self.shadow_map.as_ref()?;
        let environment = inputs.environment?;
        let primary = environment.primary;
        if !primary.casts_shadow || primary.intensity <= 0.0 || !has_casters {
            return None;
        }
        let matrix = fit_directional_shadow(
            camera.focus,
            primary.direction,
            caster_bounds,
            options.shadow_distance,
            shadow_map.size,
        );
        let uniforms = CasterUniforms { light_view_projection: matrix.to_cols_array_2d() };
        self.gpu.queue().write_buffer(&self.caster_buffer, 0, bytemuck::bytes_of(&uniforms));
        Some(matrix)
    }

    /// Pack camera, lighting, environment, and shadow data into the frame uniform block.
    fn write_frame_uniforms(&self, inputs: &FrameInputs<'_>, camera: &CameraFrame, shadow_matrix: Option<Mat4>) {
        let mut lights = [GpuPointLight::default(); MAX_POINT_LIGHTS];
        for (slot, light) in lights.iter_mut().zip(inputs.lights.iter().take(MAX_POINT_LIGHTS)) {
            slot.position_range = light.position.extend(light.range).to_array();
            slot.color = [light.color[0], light.color[1], light.color[2], 0.0];
        }
        let (primary_direction, primary_color, skylight) = match inputs.environment {
            Some(environment) => {
                let primary = environment.primary;
                (
                    primary.direction.extend(primary.intensity).to_array(),
                    [primary.color[0], primary.color[1], primary.color[2], 0.0],
                    [
                        environment.skylight_color[0],
                        environment.skylight_color[1],
                        environment.skylight_color[2],
                        environment.skylight_intensity,
                    ],
                )
            }
            None => ([0.0, 1.0, 0.0, 0.0], [0.0; 4], [0.0; 4]),
        };
        let shadow_parameters = match (self.options.shadows, &self.shadow_map) {
            (Some(options), Some(shadow_map)) => {
                [options.normal_bias, 1.0 / shadow_map.size as f32, options.filter_radius, 0.0]
            }
            _ => [0.0; 4],
        };
        let light_count = inputs.lights.len().min(MAX_POINT_LIGHTS) as f32;
        let uniforms = FrameUniforms {
            view_projection: camera.view_projection().to_cols_array_2d(),
            view: camera.view.to_cols_array_2d(),
            eye: camera.eye.extend(inputs.time_seconds).to_array(),
            lighting: [light_count, inputs.resolved_ambient(), if shadow_matrix.is_some() { 1.0 } else { 0.0 }, 0.0],
            primary_direction,
            primary_color,
            skylight,
            shadow_view_projection: shadow_matrix.unwrap_or(Mat4::IDENTITY).to_cols_array_2d(),
            shadow_parameters,
            lights,
        };
        self.gpu.queue().write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Pack the sky's inverse camera and environment block.
    fn write_atmosphere_uniforms(&self, inputs: &FrameInputs<'_>, camera: &CameraFrame) {
        let Some(environment) = inputs.environment else {
            return;
        };
        let (width, height) = self.physical_size;
        let uniforms = AtmosphereUniforms {
            inverse_view_projection: camera.view_projection().inverse().to_cols_array_2d(),
            camera_position: camera.eye.extend(0.0).to_array(),
            viewport: [width as f32, height as f32, 1.0 / width as f32, 1.0 / height as f32],
            environment: environment.pack(),
        };
        self.gpu.queue().write_buffer(&self.atmosphere_buffer, 0, bytemuck::bytes_of(&uniforms));
    }
}

/// Pack an instance's transform, tint, opacity, and shadow flag.
fn instance_raw(instance: &MeshInstance, model: Mat4) -> InstanceRaw {
    InstanceRaw {
        model: model.to_cols_array_2d(),
        tint: [instance.tint[0], instance.tint[1], instance.tint[2], instance.opacity.clamp(0.0, 1.0)],
        flags: [if instance.receives_shadow { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
    }
}

/// The opaque index range of an asset, or all indices when it has none.
fn opaque_range(submeshes: &[Submesh]) -> std::ops::Range<u32> {
    let opaque = submeshes.iter().find(|submesh| !submesh.blend);
    match opaque {
        Some(submesh) => submesh.indices.clone(),
        None => submeshes
            .first()
            .map_or(0..0, |first| first.indices.start..submeshes.last().map_or(0, |last| last.indices.end)),
    }
}

/// Convert transient triangles to mesh vertices; incomplete trailing vertices are dropped.
fn transient_vertices(opaque: &[ColorVertex], transparent: &[ColorVertex]) -> Vec<MeshVertex> {
    let complete = |vertices: &[ColorVertex]| vertices.len() / 3 * 3;
    opaque[..complete(opaque)]
        .iter()
        .chain(&transparent[..complete(transparent)])
        .map(|vertex| MeshVertex::new(vertex.position, [0.0; 3], vertex.color, VertexMaterial::default()))
        .collect()
}

/// Reject lights the shaders cannot attenuate safely, matching the TypeScript validation.
fn validate_lights(inputs: &FrameInputs<'_>) -> Result<(), RenderError> {
    if inputs.lights.len() > MAX_POINT_LIGHTS {
        return Err(RenderError::InvalidFrame(format!("at most {MAX_POINT_LIGHTS} point lights are supported")));
    }
    for light in inputs.lights {
        let finite =
            light.position.is_finite() && light.color.iter().all(|value| value.is_finite()) && light.range.is_finite();
        if !finite || light.range <= 0.0 || light.color.iter().any(|value| *value < 0.0) {
            return Err(RenderError::InvalidFrame(
                "point lights need finite values, nonnegative color, and positive range".into(),
            ));
        }
    }
    let ambient = inputs.resolved_ambient();
    if !ambient.is_finite() || ambient < 0.0 {
        return Err(RenderError::InvalidFrame("ambient irradiance must be finite and nonnegative".into()));
    }
    Ok(())
}

/// Conservative clip-space test of transformed local bounds against the six frustum planes (zero-to-one depth).
fn outside_frustum(bounds: Bounds, model: Mat4, view_projection: Mat4) -> bool {
    let matrix = view_projection * model;
    let mut outside = [true; 6];
    for corner in bounds.corners() {
        let clip = matrix * corner.extend(1.0);
        let distances = [clip.x + clip.w, clip.w - clip.x, clip.y + clip.w, clip.w - clip.y, clip.z, clip.w - clip.z];
        for (plane, distance) in distances.iter().enumerate() {
            outside[plane] = outside[plane] && *distance < 0.0;
        }
    }
    outside.iter().any(|plane| *plane)
}

/// Transform local bounds into conservative world bounds.
fn world_bounds(bounds: Bounds, model: Mat4) -> Bounds {
    bounds.corners().iter().fold(
        Bounds { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) },
        |result, corner| {
            let point = model.transform_point3(*corner);
            Bounds { min: result.min.min(point), max: result.max.max(point) }
        },
    )
}

/// Smallest bounds containing both inputs.
fn union(left: Bounds, right: Bounds) -> Bounds {
    Bounds { min: left.min.min(right.min), max: left.max.max(right.max) }
}

fn uniform_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn vertex_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn single_buffer_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
    })
}

fn depth_texture(device: &wgpu::Device, label: &str, size: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SHADOW_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn receiver_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow receiver"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_culling_rejects_geometry_behind_the_camera() {
        let bounds = Bounds { min: Vec3::splat(-0.5), max: Vec3::splat(0.5) };
        let view = crate::camera::look_at(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y);
        let projection = crate::camera::perspective(1.0, 1.0, 0.1, 100.0);
        let view_projection = projection * view;
        assert!(!outside_frustum(bounds, Mat4::IDENTITY, view_projection));
        assert!(outside_frustum(bounds, Mat4::from_translation(Vec3::new(0.0, 0.0, 20.0)), view_projection));
        assert!(outside_frustum(bounds, Mat4::from_translation(Vec3::new(50.0, 0.0, 0.0)), view_projection));
    }
}
