// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame orchestrator: owns pipelines, shared targets, GPU mesh uploads, and pass order.
//!
//! One frame proceeds as: validate inputs, build camera matrices, cull and batch instances, upload instance and frame
//! data, render the shadow views (directional cascades and point-light faces), render the scene (sky, opaque batches, transient opaque triangles, sorted
//! transparency, then the caller's pre-sorted transparent draws such as particle batches) into the HDR target, and run
//! post-processing into the caller's presentation view. Materials define shading; the renderer defines frame-wide
//! invariants and submission order.
//!
//! With scene effects enabled on WebGPU-class backends, the scene pass splits: opaque geometry, then
//! the attachments the effects need (normals and roughness, motion vectors, clustered lights), then the effect chains
//! over the opaque HDR color, then transparency against the same depth. Temporal antialiasing jitters the raster
//! projection by a Halton sequence; history resets on camera cuts, time jumps, and projection changes.

use joy_core::LinearColor;
use std::collections::HashMap;
use std::sync::Weak;

use crate::post_process::PostProcessSettings;
use joy_core::math::{
    Mat4,
    Vec2,
    Vec3,
};
use wgpu::util::DeviceExt;

use crate::RenderError;
use crate::camera::{
    Camera,
    CameraFrame,
    WorldRay,
    intersect_ray_plane,
    project_to_viewport,
    screen_ray,
};
use crate::environment::{
    EnvironmentMap,
    EnvironmentMapOptions,
    EnvironmentPixels,
    environment_format,
    image_lighting_group,
    lighting_textures,
    lighting_views,
    prepare_environment_map,
};
use crate::frame::{
    ColorVertex,
    FrameInputs,
    MAX_POINT_LIGHTS,
    sort_transparent_triangles,
};
use crate::gpu::GpuContext;
use crate::materials::{
    AtmosphereUniforms,
    CasterBias,
    FrameUniforms,
    GpuPointLight,
    InstanceRaw,
    MaterialMaps,
    PlannedDraw,
    ScenePipelines,
    TexturedPass,
};
use crate::mesh::{
    Bounds,
    MaterialTextures,
    MeshAsset,
    MeshVertex,
    Submesh,
    VertexMaterial,
};
use crate::postprocessor::PostProcessor;
use crate::scene::{
    MeshInstance,
    MeshInstanceId,
    RenderScene,
    Shading,
};
use crate::scene_effects::{
    EffectsFrame,
    MotionPass,
    MotionUniforms,
    SceneEffectsCapabilities,
    SceneEffectsError,
    SceneEffectsOptions,
    SceneEffectsRuntime,
    SceneVolumeStats,
    SurfacePass,
    VolumeCamera,
    VolumeLights,
    capabilities,
};
use crate::shadows::{
    CASCADE_BLEND_FRACTION,
    CASCADE_TEXEL_BIAS,
    FAR_FADE_FRACTION,
    POINT_FACES,
    POINT_SOURCE_RADIUS,
    PointShadowRaw,
    ShadowMaps,
    ShadowOptions,
    ShadowUniforms,
    directional_cascades,
    point_face_views,
    point_near_plane,
    practical_cascade_splits,
};
use crate::terrain::terrain_scene_pass::SceneTerrain;

/// Initial instance buffer capacity; grows geometrically.
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Initial transient vertex capacity; grows geometrically.
const INITIAL_TRANSIENT_CAPACITY: usize = 1024;

/// Renderer construction settings. The renderer copies them.
#[derive(Clone, Debug, PartialEq)]
pub struct RendererOptions {
    /// Directional shadows from the environment's primary light and point-light shadows; `None` disables shadow work.
    pub shadows: Option<ShadowOptions>,
    /// Draw the procedural sky when an environment is supplied.
    pub atmosphere: bool,
    /// Post-processing settings; replace later with [`Renderer::set_post_process`].
    pub post_process: PostProcessSettings,
    /// Opaque-scene effects (ambient occlusion, reflections, antialiasing, ...); replace later with
    /// [`Renderer::set_scene_effects`]. Ignored on backends without scene effects (WebGL 2).
    pub scene_effects: Option<SceneEffectsOptions>,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self {
            shadows: Some(ShadowOptions::default()),
            atmosphere: true,
            post_process: PostProcessSettings::DEFAULT,
            scene_effects: None,
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
    /// Instances rendered into the shadow maps.
    pub shadow_casters: usize,
    /// Whether directional shadow cascades were rendered this frame.
    pub shadows_active: bool,
    /// Shadow views rendered this frame: cascades plus six faces per shadowed point light.
    pub shadow_views: usize,
    /// Point lights that cast shadows this frame.
    pub shadow_point_lights: usize,
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
    /// Scene identity for motion vectors; shadow casters leave it unset.
    id: Option<MeshInstanceId>,
}

/// The last submitted camera, for history resets and reprojection.
struct PreviousCamera {
    eye: Vec3,
    focus: Vec3,
    projection: Mat4,
    view_projection: Mat4,
    raster_view_projection: Mat4,
    jitter: [f32; 2],
    time: f32,
}

/// Scene effects and the frame-to-frame state they need.
struct EffectsState {
    runtime: SceneEffectsRuntime,
    surfaces: Option<SurfacePass>,
    motion: Option<MotionPass>,
    volume: Option<VolumeLights>,
    previous_models: HashMap<MeshInstanceId, Mat4>,
    previous_camera: Option<PreviousCamera>,
}

impl EffectsState {
    fn reset_history(&mut self) {
        self.runtime.reset_history();
        self.previous_models.clear();
        self.previous_camera = None;
    }
}

/// Temporal-antialiasing jitter samples before the Halton sequence repeats.
const JITTER_SAMPLE_COUNT: u32 = 8;

/// One blended instance range waiting to be sorted.
struct BlendItem {
    distance: f32,
    pipeline: BlendPipeline,
    asset: MeshAsset,
    indices: std::ops::Range<u32>,
    material: Option<u64>,
    raw: InstanceRaw,
}

/// A recorded batch: one asset submesh drawn for a contiguous instance range.
/// Shadow-caster batches share this record; the caster pass ignores `pipeline`.
struct Batch {
    pipeline: OpaquePipeline,
    asset_id: u64,
    indices: std::ops::Range<u32>,
    instances: std::ops::Range<u32>,
    /// Material maps for the PBR pass and masked casters; `None` binds the white default.
    material: Option<u64>,
    /// Whether shadow casters must alpha-test this range.
    masked: bool,
}

/// Transparent work recorded after opaque geometry (and after effects, when they run).
struct TransparentWork<'a> {
    blend_items: &'a [BlendItem],
    blend_start: u32,
    transient: std::ops::Range<u32>,
    plan: &'a [PlannedDraw],
}

/// Frame values the effect attachments and chains read.
struct EffectsContext<'a> {
    inputs: &'a FrameInputs<'a>,
    camera: &'a CameraFrame,
    raster: &'a CameraFrame,
    opaque_batches: &'a [Batch],
    transient_opaque_count: u32,
    instance_count: usize,
    models: &'a [(MeshInstanceId, Mat4)],
    jitter: [f32; 2],
    size: (u32, u32),
}

/// GPU copy of one asset, released when the asset is dropped everywhere.
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    asset: Weak<dyn std::any::Any + Send + Sync>,
}

/// Shadow views and receiver data prepared for one frame.
struct ShadowPlan {
    /// Light view-projections in pass order: cascades, then six faces per shadowed point light.
    views: Vec<Mat4>,
    cascades: usize,
    point_lights: usize,
    /// Per point light, its shadow slot plus one, or zero (`PointLight.color.a` in scene-common.wgsl).
    slots: [f32; MAX_POINT_LIGHTS],
}

/// The engine's renderer. Borrows scenes and cameras per frame; owns all GPU resources it creates.
pub struct Renderer {
    // Pipelines and per-frame buffers.
    pipelines: ScenePipelines,
    post: PostProcessor,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    atmosphere_buffer: wgpu::Buffer,
    atmosphere_bind_group: wgpu::BindGroup,
    shadows: ShadowMaps,
    // PBR material maps and image-based lighting (the default group is black with zero intensity).
    material_maps: MaterialMaps,
    image_lighting_buffer: wgpu::Buffer,
    default_image_lighting: wgpu::BindGroup,
    _default_image_views: [wgpu::TextureView; 3],
    _default_image_textures: [wgpu::Texture; 3],
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    transient_buffer: wgpu::Buffer,
    transient_capacity: usize,
    // Textured transparent material and its uploaded textures (FrameInputs::transparent_draws).
    textured: TexturedPass,
    // Scene effects, when enabled and supported.
    effects: Option<EffectsState>,
    // Resident terrain adapters for the scene's terrain instances.
    terrain: SceneTerrain,

    // Uploaded mesh copies keyed by asset identity.
    meshes: HashMap<u64, GpuMesh>,

    // Configuration and drawing size.
    options: RendererOptions,
    physical_size: (u32, u32),
    scale_factor: f32,
    diagnostics: RendererDiagnostics,

    // The shared device; resources above keep it alive internally, so field order is not load-bearing here.
    gpu: GpuContext,
    output_format: wgpu::TextureFormat,
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
        let textured =
            TexturedPass::new(device, &pipelines.frame_layout, &pipelines.shadow_layout, capabilities.scene_format);
        let post = PostProcessor::new(device, capabilities.scene_format, output_format, options.post_process.clone());

        let frame_buffer = uniform_buffer(device, "frame uniforms", std::mem::size_of::<FrameUniforms>());
        let frame_bind_group = single_buffer_group(device, "frame", &pipelines.frame_layout, &frame_buffer);
        let atmosphere_buffer =
            uniform_buffer(device, "atmosphere uniforms", std::mem::size_of::<AtmosphereUniforms>());
        let atmosphere_bind_group =
            single_buffer_group(device, "atmosphere", &pipelines.atmosphere_layout, &atmosphere_buffer);

        let shadows = ShadowMaps::new(
            device,
            &pipelines.shadow_layout,
            &pipelines.caster_layout,
            options.shadows,
            capabilities.max_texture_dimension,
        );

        let material_maps = MaterialMaps::new(device, gpu.queue(), &pipelines.material_layout);
        let image_lighting_buffer = uniform_buffer(device, "image lighting", 16);
        let default_image_textures =
            lighting_textures(device, environment_format(capabilities.scene_format), 1, 1, 1, 1);
        let default_image_views = lighting_views(&default_image_textures);
        let default_image_lighting = image_lighting_group(
            device,
            &pipelines.image_lighting_layout,
            &image_lighting_buffer,
            &default_image_views,
        );

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

        let mut renderer = Self {
            pipelines,
            post,
            frame_buffer,
            frame_bind_group,
            atmosphere_buffer,
            atmosphere_bind_group,
            shadows,
            material_maps,
            image_lighting_buffer,
            default_image_lighting,
            _default_image_views: default_image_views,
            _default_image_textures: default_image_textures,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            transient_buffer,
            transient_capacity: INITIAL_TRANSIENT_CAPACITY,
            textured,
            effects: None,
            terrain: SceneTerrain::new(bias),
            meshes: HashMap::new(),
            options: options.clone(),
            physical_size: (0, 0),
            scale_factor: 1.0,
            diagnostics,
            gpu: gpu.clone(),
            output_format,
        };
        if let Err(error) = renderer.set_scene_effects(options.scene_effects) {
            log::error!("Scene effects are disabled: {error}");
        }
        renderer
    }

    /// Whether this backend runs scene effects (WebGPU-class backends; not WebGL 2).
    fn effects_supported(&self) -> bool {
        self.gpu.capabilities().backend != crate::GpuBackend::Gl
    }

    /// Replace the scene effects; `None` or an all-off configuration disables them. Validation failures keep the
    /// current effects. Replacing effects restarts their history.
    pub fn set_scene_effects(&mut self, options: Option<SceneEffectsOptions>) -> Result<(), SceneEffectsError> {
        if let Some(options) = &options {
            options.validate()?;
            let scene_format = self.gpu.capabilities().scene_format;
            let requested = options.color_format.map(|format| match format {
                crate::scene_effects::SceneEffectsColorFormat::Rgba16float => wgpu::TextureFormat::Rgba16Float,
                crate::scene_effects::SceneEffectsColorFormat::Rgba8unorm => wgpu::TextureFormat::Rgba8Unorm,
            });
            if requested == Some(wgpu::TextureFormat::Rgba16Float) && scene_format != wgpu::TextureFormat::Rgba16Float {
                return Err(SceneEffectsError::UnsupportedColorFormat);
            }
        }
        self.options.scene_effects = options;
        self.effects = match options {
            Some(options) if options.any_enabled() && self.effects_supported() => Some(EffectsState {
                runtime: SceneEffectsRuntime::new(self.gpu.device(), options, self.gpu.capabilities().scene_format),
                surfaces: None,
                motion: None,
                volume: None,
                previous_models: HashMap::new(),
                previous_camera: None,
            }),
            _ => None,
        };
        Ok(())
    }

    /// Integrate an equirectangular environment into image-based lighting for PBR surfaces.
    ///
    /// Submits GPU work; call during setup or when the environment changes, then pass the map each frame through
    /// [`FrameInputs::image_lighting`]. Fails for malformed or oversized pixels.
    pub fn prepare_environment_map(
        &self,
        pixels: &EnvironmentPixels<'_>,
        options: &EnvironmentMapOptions,
    ) -> Result<EnvironmentMap, RenderError> {
        prepare_environment_map(
            &self.gpu,
            &self.pipelines.image_lighting_layout,
            &self.image_lighting_buffer,
            pixels,
            options,
        )
    }

    /// The configured scene effects.
    pub fn scene_effects(&self) -> Option<&SceneEffectsOptions> {
        self.options.scene_effects.as_ref()
    }

    /// Which configured effects run on this device.
    pub fn scene_effects_capabilities(&self) -> SceneEffectsCapabilities {
        let options = self.options.scene_effects.unwrap_or_default();
        capabilities(&options, self.effects_supported(), Some(self.gpu.capabilities().scene_format))
    }

    /// Clustered volume-light statistics from the latest frame, when clustered lighting runs.
    pub fn scene_volume_stats(&self) -> Option<SceneVolumeStats> {
        self.effects.as_ref().and_then(|effects| effects.volume.as_ref()).map(VolumeLights::stats)
    }

    /// Restart temporal effect history, for camera cuts, teleports, and other discontinuities the renderer cannot see.
    pub fn reset_history(&mut self) {
        if let Some(effects) = &mut self.effects {
            effects.reset_history();
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

    /// Current post-processing settings.
    pub fn post_process(&self) -> &PostProcessSettings {
        self.post.settings()
    }

    /// Replace post-processing settings for the next frame. Cheap enough to call every frame (for example with
    /// settings resolved from post-process volumes at the camera); only changes to the effect graph's shape (bloom,
    /// blur, or lens streaks turning on or off, or a new bloom quality) reallocate targets.
    pub fn set_post_process(&mut self, settings: PostProcessSettings) {
        if self.options.post_process != settings {
            self.options.post_process = settings.clone();
            self.post.set_settings(settings);
        }
    }

    /// Whether the procedural sky draws behind scenes that supply an environment ([`RendererOptions::atmosphere`]).
    pub fn atmosphere(&self) -> bool {
        self.options.atmosphere
    }

    /// Draw (or stop drawing) the procedural sky from the next frame. Tools that light a studio preview with an
    /// environment's primary light over a plain background turn it off; hosts that share one renderer between tools
    /// re-apply it when a tool becomes active.
    pub fn set_atmosphere(&mut self, enabled: bool) {
        self.options.atmosphere = enabled;
    }

    /// The current shadow settings; `None` when shadows are off.
    pub fn shadow_options(&self) -> Option<ShadowOptions> {
        self.options.shadows
    }

    /// Replace shadow settings from the next frame; `None` disables shadow work.
    ///
    /// Distances and the receiver normal bias apply immediately, so a preview can fit them to its camera every frame.
    /// Changing the quality or map sizes (or turning shadows on or off) reallocates the shadow maps. The caster raster
    /// bias (`depth_bias` and `depth_bias_slope_scale`) is compiled into the caster pipelines and stays at the values
    /// the renderer was created with.
    pub fn set_shadow_options(&mut self, options: Option<ShadowOptions>) {
        let max_texture_dimension = self.gpu.capabilities().max_texture_dimension;
        let layout = |options: Option<ShadowOptions>| {
            options.map(|options| {
                (
                    options.quality,
                    options.directional_map_size(max_texture_dimension),
                    options.point_map_size(max_texture_dimension),
                )
            })
        };
        if layout(options) != layout(self.options.shadows) {
            self.shadows = ShadowMaps::new(
                self.gpu.device(),
                &self.pipelines.shadow_layout,
                &self.pipelines.caster_layout,
                options,
                max_texture_dimension,
            );
        }
        self.options.shadows = options;
    }

    /// Statistics from the most recent frame.
    pub fn diagnostics(&self) -> &RendererDiagnostics {
        &self.diagnostics
    }

    /// The device this renderer draws with.
    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// Format of the views this renderer presents into; other renderers drawing into the same target must match it.
    pub fn output_format(&self) -> wgpu::TextureFormat {
        self.output_format
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
        let mut encoder =
            self.gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("render frame") });
        self.encode(&mut encoder, scene, camera, inputs, output)?;
        self.gpu.queue().submit([encoder.finish()]);
        Ok(())
    }

    /// Record one frame into `encoder` without submitting, so a host can append overlays (such as development UI).
    ///
    /// Uploads happen immediately through the queue. Fails without recording anything for a zero-sized drawing buffer,
    /// invalid lights, or a transparent-draw texture larger than the device supports.
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

        // Camera and culling. Effects may jitter the raster projection; culling and shadows use the stable camera.
        let camera_frame = self.camera_frame(camera);
        let view_projection = camera_frame.view_projection();
        let logical_height = self.physical_size.1 as f32 / self.scale_factor;
        let terrain =
            self.terrain.prepare(&self.gpu, &self.pipelines, encoder, scene, &camera_frame, logical_height)?;
        let mut effects = self.effects.take();
        let (raster_frame, jitter) = match effects.as_mut() {
            Some(effects) => self.prepare_effects_camera(effects, &camera_frame, inputs.time_seconds),
            None => (camera_frame, [0.0; 2]),
        };
        let mut visible = terrain.visible;
        let mut culled = terrain.culled;
        let mut opaque_items = Vec::new();
        let mut blend_items = Vec::new();
        let mut caster_items = Vec::new();
        let mut caster_bounds: Option<Bounds> = terrain.caster_bounds;

        for (id, instance) in scene.iter() {
            let (model, shown) = scene.compose(instance.parent, instance.transform.matrix(), instance.visible);
            if !shown || instance.opacity <= 0.0 {
                continue;
            }
            let raw = instance_raw(instance, model);
            let transparent = instance.is_transparent();

            // Shadow casters include camera-culled opaque geometry so off-screen objects still cast into view.
            if instance.casts_shadow && !transparent {
                caster_items.push(OpaqueItem {
                    pipeline: OpaquePipeline::Lit,
                    asset: instance.asset.clone(),
                    raw,
                    id: None,
                });
                let world = world_bounds(instance.asset.bounds(), model);
                caster_bounds = Some(caster_bounds.map_or(world, |bounds| union(bounds, world)));
            }

            if outside_frustum(instance.asset.bounds(), model, view_projection) {
                culled += 1;
                continue;
            }
            visible += 1;
            self.queue_instance(
                id,
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
        let mut transient = transient_vertices(
            inputs.opaque_triangles,
            &sort_transparent_triangles(inputs.transparent_triangles, backward),
        );
        let transient_opaque_count = (inputs.opaque_triangles.len() / 3 * 3) as u32;
        let transient_transparent_end = transient.len() as u32;

        // Caller-sorted transparent runs: colored vertices follow the transient stream, textured ones upload here.
        let transparent_plan = match self.textured.prepare(&self.gpu, inputs.transparent_draws, &mut transient) {
            Ok(plan) => plan,
            Err(error) => {
                self.effects = effects;
                return Err(error);
            }
        };

        // Ensure every referenced asset and material has a GPU copy before passes open.
        for item in opaque_items.iter().chain(&caster_items) {
            self.upload_mesh(&item.asset);
        }
        for item in &blend_items {
            self.upload_mesh(&item.asset);
        }
        let assets = opaque_items.iter().chain(&caster_items).map(|item| &item.asset);
        let materials: Vec<MaterialTextures> = assets
            .chain(blend_items.iter().map(|item| &item.asset))
            .flat_map(|asset| asset.submeshes().iter().filter_map(|submesh| submesh.textures.clone()))
            .collect();
        let max_texture_dimension = self.gpu.capabilities().max_texture_dimension;
        for material in &materials {
            if let Err(error) =
                self.material_maps.prepare(self.gpu.device(), self.gpu.queue(), material, max_texture_dimension)
            {
                self.effects = effects;
                return Err(error);
            }
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

        // Shadow views, frame uniforms, and sky uniforms. Transient opaque triangles cast like meshes.
        if let Some(bounds) = triangle_bounds(inputs.opaque_triangles) {
            caster_bounds = Some(caster_bounds.map_or(bounds, |casters| union(casters, bounds)));
        }
        let shadow_plan = self.plan_shadows(inputs, &camera_frame, caster_bounds);
        self.write_frame_uniforms(inputs, &raster_frame, &shadow_plan.slots);
        let image_parameters = inputs
            .image_lighting
            .map_or([0.0; 4], |lighting| [lighting.intensity, lighting.rotation, lighting.map.max_mip, 0.0]);
        self.gpu.queue().write_buffer(&self.image_lighting_buffer, 0, bytemuck::cast_slice(&image_parameters));
        let draw_sky = self.options.atmosphere && inputs.environment.is_some();
        if draw_sky {
            self.write_atmosphere_uniforms(inputs, &raster_frame);
        }

        // Shadow passes: one depth-only view per cascade and point-light face.
        let mut draw_calls = 0;
        for view in 0..shadow_plan.views.len() {
            let target = match view.checked_sub(shadow_plan.cascades) {
                None => self.shadows.cascade_layer(view),
                Some(face) => self.shadows.point_layer(face / POINT_FACES, face % POINT_FACES),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow view"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: target,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, self.shadows.caster(), &[ShadowMaps::caster_offset(view)]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            draw_calls += self.draw_casters(&mut pass, &caster_batches);
            if transient_opaque_count > 0 {
                pass.set_pipeline(&self.pipelines.caster);
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(0..transient_opaque_count, 0..1);
                draw_calls += 1;
            }
            draw_calls += self.terrain.draw_shadow(&mut pass);
        }

        // Scene pass into the HDR target. With effects, depth survives the opaque pass and transparency draws later.
        self.post.prepare(self.gpu.device(), self.gpu.queue(), width, height, inputs.time_seconds);
        let receiver = self.shadows.receiver();
        let image_lighting =
            inputs.image_lighting.map_or(&self.default_image_lighting, |lighting| &lighting.map.bind_group);
        let transparent = TransparentWork {
            blend_items: &blend_items,
            blend_start,
            transient: transient_opaque_count..transient_transparent_end,
            plan: &transparent_plan,
        };
        let mut triangles = 0;
        {
            let clear = inputs.clear_color;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.post.scene_view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(clear.red),
                            g: f64::from(clear.green),
                            b: f64::from(clear.blue),
                            a: f64::from(clear.alpha),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.post.depth_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: if effects.is_some() { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let (calls, drawn) = self.draw_opaque(
                &mut pass,
                receiver,
                image_lighting,
                draw_sky,
                &opaque_batches,
                transient_opaque_count,
            );
            let (terrain_calls, terrain_triangles) = self.terrain.draw_lit(&mut pass);
            draw_calls += calls + terrain_calls;
            triangles += drawn + terrain_triangles;
            if effects.is_none() {
                let (calls, drawn) = self.draw_transparent(&mut pass, &transparent);
                draw_calls += calls;
                triangles += drawn;
            }
        }

        if let Some(state) = effects.as_mut() {
            let models: Vec<(MeshInstanceId, Mat4)> = opaque_items
                .iter()
                .filter_map(|item| item.id.map(|id| (id, Mat4::from_cols_array_2d(&item.raw.model))))
                .collect();
            let context = EffectsContext {
                inputs,
                camera: &camera_frame,
                raster: &raster_frame,
                opaque_batches: &opaque_batches,
                transient_opaque_count,
                instance_count: raws.len(),
                models: &models,
                jitter,
                size: (width, height),
            };
            if let Err(error) = self.encode_effects(encoder, state, &context) {
                state.reset_history();
                self.effects = effects;
                return Err(RenderError::InvalidFrame(error.to_string()));
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene transparency"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.post.scene_view(),
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.post.depth_view(),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, receiver, &[]);
            pass.set_bind_group(2, image_lighting, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            let (calls, drawn) = self.draw_transparent(&mut pass, &transparent);
            draw_calls += calls;
            triangles += drawn;
            drop(pass);

            // Remember this frame for reprojection and cut detection.
            state.previous_models = models.into_iter().collect();
            state.previous_camera = Some(PreviousCamera {
                eye: camera_frame.eye,
                focus: camera_frame.focus,
                projection: camera_frame.projection,
                view_projection,
                raster_view_projection: raster_frame.view_projection(),
                jitter,
                time: inputs.time_seconds,
            });
        }
        self.effects = effects;

        self.post.encode(encoder, output);
        self.release_dropped_meshes();
        self.textured.release_dropped();
        self.material_maps.release_dropped();

        self.diagnostics.visible = visible;
        self.diagnostics.culled = culled;
        self.diagnostics.draw_calls = draw_calls;
        self.diagnostics.triangles = triangles;
        self.diagnostics.shadow_casters = if shadow_plan.views.is_empty() { 0 } else { caster_items.len() };
        self.diagnostics.shadows_active = shadow_plan.cascades > 0;
        self.diagnostics.shadow_views = shadow_plan.views.len();
        self.diagnostics.shadow_point_lights = shadow_plan.point_lights;
        Ok(())
    }

    /// Route one camera-visible instance into the opaque batches and/or the sorted blend queue.
    #[allow(clippy::too_many_arguments)]
    fn queue_instance(
        &self,
        id: MeshInstanceId,
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
                    material: submesh.textures.as_ref().map(MaterialTextures::id),
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
        opaque_items.push(OpaqueItem { pipeline, asset: instance.asset.clone(), raw, id: Some(id) });

        // PBR materials can blend part of an otherwise opaque asset.
        if instance.shading == Shading::Pbr {
            for submesh in instance.asset.submeshes().iter().filter(|submesh| submesh.blend) {
                blend_items.push(BlendItem {
                    distance,
                    pipeline: BlendPipeline::Pbr,
                    asset: instance.asset.clone(),
                    indices: submesh.indices.clone(),
                    material: submesh.textures.as_ref().map(MaterialTextures::id),
                    raw,
                });
            }
        }
    }

    /// Sky, opaque batches, and transient opaque triangles; returns draw calls and triangles.
    fn draw_opaque(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        receiver: &wgpu::BindGroup,
        image_lighting: &wgpu::BindGroup,
        draw_sky: bool,
        batches: &[Batch],
        transient_opaque_count: u32,
    ) -> (usize, usize) {
        let mut draw_calls = 0;
        if draw_sky {
            pass.set_pipeline(&self.pipelines.atmosphere);
            pass.set_bind_group(0, &self.atmosphere_bind_group, &[]);
            pass.draw(0..3, 0..1);
            draw_calls += 1;
        }
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, receiver, &[]);
        pass.set_bind_group(2, image_lighting, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));

        // Opaque batches, grouped by pipeline; PBR batches bind their material maps.
        let mut triangles = 0;
        let mut bound = None;
        for batch in batches {
            if bound != Some(batch.pipeline) {
                pass.set_pipeline(self.opaque_pipeline(batch.pipeline));
                bound = Some(batch.pipeline);
            }
            if batch.pipeline == OpaquePipeline::Pbr {
                pass.set_bind_group(3, self.material_maps.group(batch.material), &[]);
            }
            triangles += self.draw_batch(pass, batch);
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
        (draw_calls, triangles)
    }

    /// Sorted blended ranges, sorted transient transparency, then caller-sorted transparent runs; returns draw calls and
    /// triangles. Expects groups 0 and 1 and the instance buffer bound.
    fn draw_transparent(&self, pass: &mut wgpu::RenderPass<'_>, work: &TransparentWork<'_>) -> (usize, usize) {
        let (mut draw_calls, mut triangles) = (0, 0);
        for (offset, item) in work.blend_items.iter().enumerate() {
            let Some(mesh) = self.meshes.get(&item.asset.id()) else {
                continue;
            };
            match item.pipeline {
                BlendPipeline::Unlit => pass.set_pipeline(&self.pipelines.unlit_blend),
                BlendPipeline::Pbr => {
                    pass.set_pipeline(&self.pipelines.pbr_blend);
                    pass.set_bind_group(3, self.material_maps.group(item.material), &[]);
                }
            }
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            let instance = work.blend_start + offset as u32;
            pass.draw_indexed(item.indices.clone(), 0, instance..instance + 1);
            draw_calls += 1;
            triangles += item.indices.len() / 3;
        }
        if !work.transient.is_empty() {
            pass.set_pipeline(&self.pipelines.unlit_blend);
            pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
            pass.draw(work.transient.clone(), 0..1);
            draw_calls += 1;
            triangles += work.transient.len() / 3;
        }

        // Caller-sorted transparent runs, in the supplied order (groups 0 and 1 stay bound across pipelines).
        for planned in work.plan {
            match planned {
                PlannedDraw::Colored(range) => {
                    pass.set_pipeline(&self.pipelines.unlit_blend);
                    pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                    pass.draw(range.clone(), 0..1);
                    triangles += range.len() / 3;
                }
                PlannedDraw::Textured { texture, vertices } => {
                    if !self.textured.draw(pass, *texture, vertices.clone()) {
                        continue;
                    }
                    triangles += vertices.len() / 3;
                }
            }
            draw_calls += 1;
        }
        (draw_calls, triangles)
    }

    /// Detect history discontinuities and jitter the raster projection for temporal antialiasing.
    fn prepare_effects_camera(
        &self,
        effects: &mut EffectsState,
        camera: &CameraFrame,
        time: f32,
    ) -> (CameraFrame, [f32; 2]) {
        if let Some(previous) = &effects.previous_camera {
            let distance = (camera.eye - camera.focus).length();
            let previous_forward = (previous.focus - previous.eye).normalize_or_zero();
            let forward = (camera.focus - camera.eye).normalize_or_zero();
            let time_jump = time < previous.time || time - previous.time > 0.25;
            let cut = camera.eye.distance(previous.eye) > distance * 0.5
                || previous_forward.dot(forward) < 0.5_f32.cos()
                || camera.projection != previous.projection;
            if time_jump || cut {
                effects.reset_history();
            }
        }
        if effects.runtime.options().taa.is_none() {
            return (*camera, [0.0; 2]);
        }
        let (width, height) = self.physical_size;
        let jitter = scene_camera_jitter(effects.runtime.frame_index(), width, height);
        (CameraFrame { projection: jitter_projection(camera.projection, jitter), ..*camera }, jitter)
    }

    /// Render the attachments the effects need, then run the effect chains over the opaque scene color.
    fn encode_effects(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        state: &mut EffectsState,
        context: &EffectsContext<'_>,
    ) -> Result<(), SceneEffectsError> {
        let device = self.gpu.device();
        let queue = self.gpu.queue();
        let requirements = state.runtime.options().requirements();
        let raster_view_projection = context.raster.view_projection();
        let view_projection = context.camera.view_projection();

        if requirements.normal_texture {
            let surfaces = state.surfaces.get_or_insert_with(|| SurfacePass::new(device, &self.pipelines.frame_layout));
            let mut pass = surfaces.begin(device, encoder, self.post.depth_view(), context.size);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            for batch in context.opaque_batches {
                self.draw_batch(&mut pass, batch);
            }
            if context.transient_opaque_count > 0 {
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(0..context.transient_opaque_count, 0..1);
            }
            self.terrain.draw_surface(&mut pass);
        } else {
            state.surfaces = None;
        }

        let previous = state.previous_camera.as_ref();
        let previous_jitter = previous.map_or([0.0; 2], |previous| previous.jitter);
        let current_jitter = context.jitter;
        if requirements.velocity_texture || requirements.taa_velocity_texture {
            // Previous models line up with the instance buffer: identity, then camera-opaque instances in batch order.
            let mut previous_models = Vec::with_capacity(context.instance_count);
            previous_models.push(Mat4::IDENTITY);
            for (id, model) in context.models {
                previous_models.push(state.previous_models.get(id).copied().unwrap_or(*model));
            }
            previous_models.resize(context.instance_count.max(previous_models.len()), Mat4::IDENTITY);
            let uniforms = MotionUniforms {
                current_view_projection: view_projection.to_cols_array_2d(),
                previous_view_projection: previous
                    .map_or(view_projection, |previous| previous.view_projection)
                    .to_cols_array_2d(),
                raster_view_projection: raster_view_projection.to_cols_array_2d(),
                velocity_jitter_delta: [
                    current_jitter[0] - previous_jitter[0],
                    current_jitter[1] - previous_jitter[1],
                    0.0,
                    0.0,
                ],
            };
            let motion = state.motion.get_or_insert_with(|| MotionPass::new(device));
            let mut pass = motion.begin(device, queue, encoder, &uniforms, &previous_models, context.size);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            for batch in context.opaque_batches {
                self.draw_batch(&mut pass, batch);
            }
            if context.transient_opaque_count > 0 {
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(0..context.transient_opaque_count, 0..1);
            }
            self.terrain.draw_motion(queue, &mut pass, &uniforms);
        } else {
            state.motion = None;
        }

        let clusters = if requirements.clustered_lighting {
            let volume = state.volume.get_or_insert_with(|| VolumeLights::new(device));
            let camera = VolumeCamera {
                view: context.camera.view,
                projection: context.raster.projection,
                near: context.camera.near,
                far: context.camera.far,
                width: context.size.0,
                height: context.size.1,
            };
            Some(volume.update(device, queue, context.inputs.lights, &camera))
        } else {
            state.volume = None;
            None
        };

        let inverse_raster = raster_view_projection.inverse();
        let (direction, color) = match context.inputs.environment {
            Some(environment) => {
                let primary = environment.primary;
                let direction = context.camera.view.transform_vector3(primary.direction);
                (direction.to_array(), primary.color.scaled(primary.intensity).rgb_array())
            }
            None => ([0.0, 1.0, 0.0], [0.0; 3]),
        };
        let frame = EffectsFrame {
            depth: self.post.depth_view(),
            normal: state.surfaces.as_ref().and_then(SurfacePass::view),
            velocity: state.motion.as_ref().and_then(MotionPass::jittered),
            taa_velocity: state.motion.as_ref().and_then(MotionPass::unjittered),
            projection: context.raster.projection,
            inverse_projection: context.raster.projection.inverse(),
            inverse_view: context.camera.view.inverse(),
            inverse_view_projection: view_projection.inverse(),
            previous_view_projection: previous.map_or(view_projection, |previous| previous.view_projection),
            inverse_raster_view_projection: inverse_raster,
            previous_raster_view_projection: previous
                .map_or(raster_view_projection, |previous| previous.raster_view_projection),
            near: context.camera.near,
            far: context.camera.far,
            orthographic: context.camera.projection.w_axis.w != 0.0,
            current_jitter,
            previous_jitter: previous.map_or(current_jitter, |previous| previous.jitter),
            directional_direction_view: direction,
            directional_color: color,
            clusters,
            time_seconds: context.inputs.time_seconds,
            delta_time_seconds: previous.map_or(0.0, |previous| (context.inputs.time_seconds - previous.time).max(0.0)),
            reset_history: previous.is_none(),
        };
        state.runtime.encode(
            device,
            queue,
            encoder,
            self.post.scene_view(),
            self.gpu.capabilities().scene_format,
            context.size,
            &frame,
        )
    }

    /// Append sorted items to the instance list and group runs of the same pipeline and asset into batches.
    /// Each batch draws the asset's opaque submesh (or all of it when the asset has no opaque range).
    fn batch(&self, items: &[OpaqueItem], raws: &mut Vec<InstanceRaw>) -> Vec<Batch> {
        let mut batches: Vec<Batch> = Vec::new();
        // First batch of the current asset; an asset with several opaque submeshes records one batch per submesh.
        let mut group_start = 0;
        for item in items {
            let instance = raws.len() as u32;
            raws.push(item.raw);
            let pipeline = item.pipeline;
            if let Some(last) = batches.last()
                && last.pipeline == pipeline
                && last.asset_id == item.asset.id()
            {
                for batch in &mut batches[group_start..] {
                    batch.instances.end = instance + 1;
                }
                continue;
            }
            group_start = batches.len();
            let instances = instance..instance + 1;
            for (indices, submesh) in opaque_ranges(item.asset.submeshes()) {
                batches.push(Batch {
                    pipeline,
                    asset_id: item.asset.id(),
                    indices,
                    instances: instances.clone(),
                    material: submesh.and_then(|submesh| submesh.textures.as_ref()).map(MaterialTextures::id),
                    masked: submesh.is_some_and(|submesh| submesh.masked),
                });
            }
        }
        batches
    }

    /// Draw caster batches with the depth-only or alpha-tested caster; returns the number of draw calls.
    fn draw_casters(&self, pass: &mut wgpu::RenderPass<'_>, batches: &[Batch]) -> usize {
        let mut masked = None;
        for batch in batches {
            if masked != Some(batch.masked) {
                pass.set_pipeline(if batch.masked { &self.pipelines.masked_caster } else { &self.pipelines.caster });
                masked = Some(batch.masked);
            }
            if batch.masked {
                pass.set_bind_group(1, self.material_maps.group(batch.material), &[]);
            }
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

    /// Choose this frame's shadow views and upload their matrices and the receiver uniforms.
    ///
    /// The primary light casts cascades when it is lit and set to cast. Point lights set to cast get six faces each when
    /// their reach touches a caster; a light whose reach holds no caster would see an empty map, so it is skipped.
    fn plan_shadows(
        &mut self,
        inputs: &FrameInputs<'_>,
        camera: &CameraFrame,
        caster_bounds: Option<Bounds>,
    ) -> ShadowPlan {
        let mut plan = ShadowPlan { views: Vec::new(), cascades: 0, point_lights: 0, slots: [0.0; MAX_POINT_LIGHTS] };
        let mut uniforms = ShadowUniforms::disabled();
        let (Some(options), Some(casters)) = (self.options.shadows, caster_bounds) else {
            self.shadows.write_uniforms(self.gpu.queue(), &uniforms);
            return plan;
        };
        let quality = options.quality;
        uniforms.counts = [
            0.0,
            quality.cascade_count() as f32,
            quality.blocker_sample_count() as f32,
            quality.filter_sample_count() as f32,
        ];

        if let Some(environment) = inputs.environment {
            let primary = environment.primary;
            let distance = options.shadow_distance.min(camera.far).max(camera.near);
            if primary.casts_shadow && primary.intensity > 0.0 && distance > camera.near {
                let count = quality.cascade_count();
                let splits = practical_cascade_splits(camera.near, distance, count);
                let map_size = options.directional_map_size(self.gpu.capabilities().max_texture_dimension);
                let cascades = directional_cascades(camera, primary.direction, &splits, count, distance, map_size);
                for (index, (matrix, texel)) in cascades.iter().enumerate() {
                    uniforms.cascades[index] = matrix.to_cols_array_2d();
                    uniforms.cascade_bias[index] = options.normal_bias.max(texel * CASCADE_TEXEL_BIAS);
                }
                uniforms.counts[0] = 1.0;
                uniforms.cascade_splits = splits;
                uniforms.directional =
                    [options.normal_bias, primary.angular_radius, CASCADE_BLEND_FRACTION, FAR_FADE_FRACTION];
                uniforms.extents[0] = distance;
                uniforms.extents[1] = 1.0;
                plan.cascades = cascades.len();
                plan.views.extend(cascades.iter().map(|(matrix, _)| *matrix));
            }
        }

        for (index, light) in inputs.lights.iter().take(MAX_POINT_LIGHTS).enumerate() {
            let nearest = light.position.clamp(casters.min, casters.max);
            if !light.casts_shadow || nearest.distance(light.position) > light.range {
                continue;
            }
            let slot = plan.point_lights;
            uniforms.points[slot] = PointShadowRaw {
                position_range: light.position.extend(light.range).to_array(),
                parameters: [point_near_plane(light.range), POINT_SOURCE_RADIUS, options.normal_bias, 1.0],
            };
            plan.slots[index] = (slot + 1) as f32;
            plan.views.extend(point_face_views(light.position, light.range));
            plan.point_lights += 1;
        }
        uniforms.extents[2] = plan.point_lights as f32;

        let device = self.gpu.device().clone();
        self.shadows.reserve_point_lights(&device, plan.point_lights);
        self.shadows.write_casters(&device, self.gpu.queue(), &plan.views);
        self.shadows.write_uniforms(self.gpu.queue(), &uniforms);
        plan
    }

    /// Pack camera, lighting, environment, and shadow data into the frame uniform block.
    fn write_frame_uniforms(
        &self,
        inputs: &FrameInputs<'_>,
        camera: &CameraFrame,
        shadow_slots: &[f32; MAX_POINT_LIGHTS],
    ) {
        let mut lights = [GpuPointLight::default(); MAX_POINT_LIGHTS];
        for ((slot, light), shadow) in
            lights.iter_mut().zip(inputs.lights.iter().take(MAX_POINT_LIGHTS)).zip(shadow_slots)
        {
            slot.position_range = light.position.extend(light.range).to_array();
            slot.color = light.color.with_alpha(*shadow).to_array();
        }
        let (primary_direction, primary_color, skylight) = match inputs.environment {
            Some(environment) => {
                let primary = environment.primary;
                (
                    primary.direction.extend(primary.intensity).to_array(),
                    primary.color.with_alpha(0.0).to_array(),
                    environment.skylight_color.with_alpha(environment.skylight_intensity).to_array(),
                )
            }
            None => ([0.0, 1.0, 0.0, 0.0], [0.0; 4], [0.0; 4]),
        };
        let light_count = inputs.lights.len().min(MAX_POINT_LIGHTS) as f32;
        let uniforms = FrameUniforms {
            view_projection: camera.view_projection().to_cols_array_2d(),
            view: camera.view.to_cols_array_2d(),
            eye: camera.eye.extend(inputs.time_seconds).to_array(),
            lighting: [light_count, inputs.resolved_ambient(), 0.0, 0.0],
            primary_direction,
            primary_color,
            skylight,
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
/// Sub-pixel jitter for one frame: a Halton (2, 3) sequence of eight samples, in UV units of the backing size.
fn scene_camera_jitter(frame_index: u32, width: u32, height: u32) -> [f32; 2] {
    let sample = (frame_index % JITTER_SAMPLE_COUNT) + 1;
    [(halton(sample, 2) - 0.5) / width.max(1) as f32, (halton(sample, 3) - 0.5) / height.max(1) as f32]
}

/// Offset a projection's clip XY by a UV jitter (Y down, matching framebuffer UVs).
fn jitter_projection(projection: Mat4, jitter: [f32; 2]) -> Mat4 {
    let mut columns = projection.to_cols_array();
    for column in 0..4 {
        let homogeneous = columns[column * 4 + 3];
        columns[column * 4] += 2.0 * jitter[0] * homogeneous;
        columns[column * 4 + 1] -= 2.0 * jitter[1] * homogeneous;
    }
    Mat4::from_cols_array(&columns)
}

/// The radical inverse of `index` in `base`.
fn halton(mut index: u32, base: u32) -> f32 {
    let mut scale = 1.0;
    let mut result = 0.0;
    while index > 0 {
        scale /= base as f32;
        result += scale * (index % base) as f32;
        index /= base;
    }
    result
}

fn instance_raw(instance: &MeshInstance, model: Mat4) -> InstanceRaw {
    InstanceRaw {
        model: model.to_cols_array_2d(),
        tint: instance.tint.with_alpha(instance.opacity.clamp(0.0, 1.0)).to_array(),
        flags: [if instance.receives_shadow { 1.0 } else { 0.0 }, instance.scene_roughness.clamp(0.0, 1.0), 0.0, 0.0],
    }
}

/// The opaque index range of an asset, or all indices when it has none.
/// Opaque index ranges with their submeshes. An asset with no opaque submesh (drawn opaque by request) draws its whole
/// index range untextured.
fn opaque_ranges(submeshes: &[Submesh]) -> Vec<(std::ops::Range<u32>, Option<&Submesh>)> {
    let opaque: Vec<_> = submeshes
        .iter()
        .filter(|submesh| !submesh.blend)
        .map(|submesh| (submesh.indices.clone(), Some(submesh)))
        .collect();
    if !opaque.is_empty() {
        return opaque;
    }
    let whole = submeshes
        .first()
        .map_or(0..0, |first| first.indices.start..submeshes.last().map_or(0, |last| last.indices.end));
    vec![(whole, None)]
}

/// Convert transient triangles to mesh vertices; incomplete trailing vertices are dropped.
fn transient_vertices(opaque: &[ColorVertex], transparent: &[ColorVertex]) -> Vec<MeshVertex> {
    let complete = |vertices: &[ColorVertex]| vertices.len() / 3 * 3;
    opaque[..complete(opaque)]
        .iter()
        .chain(&transparent[..complete(transparent)])
        .map(|vertex| {
            MeshVertex::new(vertex.position, [0.0; 3], LinearColor::from(vertex.color), VertexMaterial::default())
        })
        .collect()
}

/// Reject lights the shaders cannot attenuate safely, matching the TypeScript validation.
fn validate_lights(inputs: &FrameInputs<'_>) -> Result<(), RenderError> {
    if inputs.lights.len() > MAX_POINT_LIGHTS {
        return Err(RenderError::InvalidFrame(format!("at most {MAX_POINT_LIGHTS} point lights are supported")));
    }
    for light in inputs.lights {
        let finite = light.position.is_finite()
            && light.color.rgb_array().iter().all(|value| value.is_finite())
            && light.range.is_finite();
        if !finite || light.range <= 0.0 || light.color.rgb_array().iter().any(|value| *value < 0.0) {
            return Err(RenderError::InvalidFrame(
                "point lights need finite values, nonnegative color, and positive range".into(),
            ));
        }
    }
    let ambient = inputs.resolved_ambient();
    if !ambient.is_finite() || ambient < 0.0 {
        return Err(RenderError::InvalidFrame("ambient irradiance must be finite and nonnegative".into()));
    }
    if let Some(lighting) = inputs.image_lighting
        && (!lighting.intensity.is_finite() || lighting.intensity < 0.0 || !lighting.rotation.is_finite())
    {
        return Err(RenderError::InvalidFrame("Environment intensity must be nonnegative and rotation finite.".into()));
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
/// World bounds of transient triangles, or `None` when there are none.
fn triangle_bounds(vertices: &[ColorVertex]) -> Option<Bounds> {
    let mut points = vertices.iter().map(|vertex| Vec3::from(vertex.position));
    let first = points.next()?;
    Some(points.fold(Bounds { min: first, max: first }, |bounds, point| Bounds {
        min: bounds.min.min(point),
        max: bounds.max.max(point),
    }))
}

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

#[cfg(test)]
#[path = "../tests/unit/renderer.rs"]
mod tests;
