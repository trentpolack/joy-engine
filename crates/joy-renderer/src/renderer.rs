// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame orchestrator: owns pipelines, shared targets, GPU mesh uploads, and pass order.
//!
//! One frame proceeds as:
//! 1. Validate inputs, build the camera, prepare terrain, choose the point lights that matter most to the view, refresh
//!    the scene's render proxies for the edits since the last frame, and cull them against the camera through the
//!    scene's bounds tree ([`crate::scene`]).
//! 2. Lay out every draw before any pass opens, as [`DrawItem`]s in per-pass queues: opaque batches grouped by pipeline
//!    and asset and ordered front to back ([`SceneBatcher`]), one back-to-front transparent queue (blended meshes,
//!    transient triangles, and particle batches, which are culled against the view and sorted per particle first), and
//!    the shadow views. The [`ShadowPlanner`] decides which shadow maps render this frame (cached maps whose view and
//!    casters did not change keep their depth) and queries each rendering view's casters from the bounds tree. Then bin
//!    the point lights into light clusters, so every lit pass reads only the lights that reach each surface.
//! 3. Plan the frame's passes ([`FramePlan`]), which derives every attachment's load and store operations from the pass
//!    order, then render the shadow views that need it (each cascade, then the point-light faces into their atlas
//!    tiles in one pass), then (with volumetric fog) inject and integrate the fog volume, then the depth pre-pass
//!    (terrain first, as the largest occluder, then opaque batches front to back).
//! 4. Shade opaque surfaces into the HDR target. With deferred lighting, a G-buffer pass writes each surface's
//!    emission and indirect light plus its encoded surface, stamping its shading model's lighting slot into the
//!    stencil; one full-screen pass per model then adds direct light where the stencil matches; and a forward pass draws
//!    the opaque materials the G-buffer cannot hold. Without it (WebGL 2, debug views, or by choice) one forward pass
//!    shades everything. Either way opaque surfaces test depth less-or-equal against the pre-pass, so each visible
//!    pixel is shaded once, and the sky fills the uncovered pixels on the far plane last. With temporal antialiasing or
//!    motion blur, opaque materials also write motion into an attachment in the passes that draw them.
//! 5. Blend volumetric fog over opaque surfaces and the sky, then draw transparency (which fogs itself, lit forward),
//!    then run the composite effects (temporal antialiasing, motion blur, and adaptive exposure) and post-processing,
//!    so every post effect sees transparent surfaces. Without fog or the motion attachment, transparency draws at the
//!    end of the last opaque pass.
//! 6. Post-process into the caller's presentation view.
//!
//! Steps 3 through 5 draw at the render size: the drawing-buffer size times the render scale. The post chain upscales that
//! scene into the output, so a scale below one trades sharpness for scene, effect, and bloom work while any UI drawn
//! afterwards keeps the full output resolution.
//!
//! Materials define shading; the renderer defines frame-wide invariants and submission order. Temporal antialiasing
//! jitters the raster projection by a Halton sequence; history resets on camera cuts, presentation-clock jumps, and
//! projection changes.
//!
//! Particles join the frame in two places in this file, each marked "Particle integration": the frame body prepares
//! the batches and enters each into the transparent queue as a [`DrawGeometry::Particles`] item, and
//! `FrameResources::draw_particles` records each item inside the transparent queue's pass.

use std::collections::HashMap;
use std::sync::Weak;

use joy_core::LinearColor;
use joy_core::math::{
    Mat4,
    Vec2,
    Vec3,
};
use joy_render_types::{
    DirectionalLight,
    PointLight,
    Skylight,
};
use wgpu::util::DeviceExt;

use crate::RenderError;
use crate::buffer_overview::{
    BUFFER_OVERVIEW_GAP,
    BUFFER_OVERVIEW_TILES,
    BufferOverview,
    OverviewTile,
};
use crate::camera::{
    Camera,
    CameraFrame,
    WorldRay,
    intersect_ray_plane,
    project_to_viewport,
    screen_ray,
};
use crate::debug_view::DebugView;
use crate::deferred::GBuffer;
use crate::display_output::DisplayOutput;
use crate::draw_items::{
    DrawGeometry,
    DrawItem,
    OWN_MATERIAL,
    OpaqueQueues,
};
use crate::environment::{
    EnvironmentMap,
    EnvironmentMapOptions,
    EnvironmentPixels,
    ImageLightingResources,
};
use crate::frame::{
    ColorVertex,
    FrameInputs,
    PresentationTime,
};
use crate::frame_plan::{
    FramePass,
    FramePlan,
    FrameShape,
    FrameTargets,
    Load,
    PlannedPass,
};
use crate::gpu::GpuContext;
use crate::lighting::{
    CLUSTER_COLUMNS,
    CLUSTER_ROWS,
    CLUSTER_SLICES,
    LightClusters,
    LightTextures,
};
use crate::material::{
    LIT_MODEL_ID,
    MaterialAsset,
    MaterialCache,
    ModelId,
    ShadingModelDefinition,
    ShadingModels,
    UNLIT_MODEL_ID,
};
use crate::mesh::{
    Bounds,
    MeshAsset,
    MeshVertex,
};
use crate::pipelines::{
    AtmosphereUniforms,
    CasterBias,
    FOG_VOLUME_FORMAT,
    FrameUniforms,
    InstanceRaw,
    ModelPass,
    ParticlePass,
    ParticleView,
    ScenePipelines,
    SceneTargets,
    scene_common,
};
use crate::point_lights::select_point_lights;
use crate::post_process::PostProcessSettings;
use crate::postprocessor::PostProcessor;
use crate::profiler::{
    FramePhase,
    FrameProfile,
    FrameProfiler,
};
use crate::scene::{
    MeshInstanceId,
    RenderScene,
    TerrainInstanceId,
};
use crate::scene_batcher::{
    BlendDraw,
    CameraBatches,
    CameraFrameInputs,
    CasterFrameInputs,
    SceneBatcher,
    ViewDepth,
    instance_assets,
};
use crate::scene_effects::{
    EffectSupport,
    EffectsFrame,
    EffectsSetup,
    SceneAttachments,
    SceneEffectsCapabilities,
    SceneEffectsError,
    SceneEffectsOptions,
    SceneEffectsRuntime,
    capabilities,
    linear_sampler,
};
use crate::shadows::cache::{
    ShadowInputs,
    TerrainShadowKey,
};
use crate::shadows::planner::{
    FrameCasters,
    ShadowFrame,
    ShadowPlan,
    ShadowPlanner,
    ShadowTarget,
    ShadowView,
};
use crate::shadows::{
    ShadowMaps,
    ShadowOptions,
};
use crate::terrain::terrain_scene_pass::SceneTerrain;
use crate::terrain::{
    TerrainShadowView,
    TerrainStats,
};
use crate::texture_mips::MipmapGenerator;
use crate::transparency::{
    TransparentQueue,
    TransparentStep,
    build_transparent_queue,
};

/// Initial instance buffer capacity; grows geometrically.
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Initial transient vertex capacity; grows geometrically.
const INITIAL_TRANSIENT_CAPACITY: usize = 1024;

/// Temporal-antialiasing jitter samples before the Halton sequence repeats.
const JITTER_SAMPLE_COUNT: u32 = 8;

/// A presentation-clock gap longer than this, in seconds, restarts temporal history.
const HISTORY_TIME_JUMP_SECONDS: f64 = 0.25;

/// Smallest main-scene render scale; below a quarter of the output resolution the upscaled image is too soft to read.
pub const MIN_RENDER_SCALE: f32 = 0.25;

/// Largest main-scene render scale: the scene renders at the output resolution.
pub const MAX_RENDER_SCALE: f32 = 1.0;

/// Renderer construction settings. The renderer copies them.
#[derive(Clone, Debug, PartialEq)]
pub struct RendererOptions {
    /// Directional shadows from the frame's key light and point-light shadows; `None` disables shadow work.
    pub shadows: Option<ShadowOptions>,
    /// Draw the procedural sky when an environment is supplied.
    pub atmosphere: bool,
    /// Fill scene depth in a depth-only pass before shading, so each visible pixel is shaded once however the opaque
    /// surfaces overlap. The pass costs one more vertex transform per opaque vertex; turn it off only for scenes with
    /// almost no overdraw.
    pub depth_prepass: bool,
    /// Post-processing settings; replace later with [`Renderer::set_post_process`].
    pub post_process: PostProcessSettings,
    /// Fraction of the drawing-buffer resolution the scene renders at along each axis, from [`MIN_RENDER_SCALE`] to
    /// [`MAX_RENDER_SCALE`]; replace later with [`Renderer::set_render_scale`]. Below one, the scene, its effects,
    /// and bloom run at the reduced size and the post chain upscales the result, while any UI drawn afterwards keeps
    /// the full output resolution.
    pub render_scale: f32,
    /// Scene effects (temporal antialiasing, motion blur, adaptive exposure, and volumetric fog); replace later with
    /// [`Renderer::set_scene_effects`]. Effects a backend cannot run are skipped and reported by
    /// [`Renderer::scene_effects_capabilities`].
    pub scene_effects: Option<SceneEffectsOptions>,
    /// Shade opaque surfaces with deferred lighting where the device supports it (every backend but WebGL 2): a
    /// G-buffer pass, then one full-screen lighting pass per shading model, so the cost of many lights follows the
    /// pixels they reach. Turned off, or on WebGL 2 and with debug views, every surface shades forward with the same
    /// light clusters. Replace later with [`Renderer::set_deferred`].
    pub deferred: bool,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self {
            shadows: Some(ShadowOptions::default()),
            atmosphere: true,
            depth_prepass: true,
            post_process: PostProcessSettings::DEFAULT,
            render_scale: MAX_RENDER_SCALE,
            scene_effects: None,
            deferred: true,
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
    /// The presentation target's encoding and the HDR headroom the frame tone mapped into.
    pub display: DisplayOutput,
    /// Size in physical pixels the scene rendered at this frame: the drawing-buffer size times the render scale.
    pub render_size: (u32, u32),
    /// Instances (meshes and terrain) drawn by the camera.
    pub visible: usize,
    /// Instances skipped by frustum culling.
    pub culled: usize,
    /// Draw calls recorded for the shadow, depth pre-pass, G-buffer, deferred lighting, scene, and transparency passes.
    pub draw_calls: usize,
    /// Triangles submitted to the G-buffer, scene, and transparency passes.
    pub triangles: usize,
    /// Caster instances across every shadow view in use, after culling each view's casters to it: drawn this frame,
    /// or held by a map kept from an earlier frame.
    pub shadow_casters: usize,
    /// Whether directional shadow cascades were in use this frame.
    pub shadows_active: bool,
    /// Shadow views receivers read this frame: cascades plus six faces per shadowed point light, rendered or cached.
    pub shadow_views: usize,
    /// Shadow maps (cascades and point-light faces) rendered this frame because their view or the casters inside it
    /// changed; the rest of [`shadow_views`](Self::shadow_views) kept the depth an earlier frame rendered.
    pub shadow_maps_rendered: usize,
    /// Point lights that cast shadows this frame.
    pub shadow_point_lights: usize,
    /// Point lights drawn this frame (at most [`MAX_POINT_LIGHTS`](crate::MAX_POINT_LIGHTS)).
    pub point_lights: usize,
    /// Supplied point lights left out this frame: invalid, unable to reach the view, or beyond the cap.
    pub point_lights_omitted: usize,
    /// Light references across every light cluster: how many lights the clusters list in total.
    pub cluster_light_references: usize,
    /// Light references the clusters could not hold this frame; the weakest lights lose their cells first.
    pub cluster_references_dropped: usize,
    /// Whether opaque surfaces shaded with deferred lighting this frame.
    pub deferred: bool,
    /// Full-screen deferred lighting passes drawn this frame, one per shading model with visible surfaces.
    pub lighting_passes: usize,
    /// Opaque batches shaded forward while deferred lighting ran: materials the G-buffer cannot hold, or models
    /// without deferred support.
    pub forward_opaque_batches: usize,
    /// Submeshes left out because their material names a shading model nothing registered.
    pub unavailable_materials: usize,
    /// Particle instances drawn this frame, after culling.
    pub particles: usize,
    /// Particle batches culled because their bounds lay outside the view.
    pub particle_batches_culled: usize,
}

/// The frame's cameras.
struct FrameCameras {
    /// The camera as supplied: culling, light selection, shadows, terrain LOD, and motion vectors use it.
    stable: CameraFrame,
    /// The camera geometry is rasterized with, jittered for temporal antialiasing.
    raster: CameraFrame,
    /// The previous frame's stable view-projection and view; this frame's without history.
    previous_view_projection: Mat4,
    previous_view: Mat4,
}

/// The lights a frame draws.
struct FrameLighting {
    /// The directional light that lights and shadows surfaces, when valid.
    key: Option<DirectionalLight>,
    skylight: Skylight,
    /// Uniform ambient irradiance.
    ambient: f32,
    /// The selected point lights, strongest first.
    points: Vec<PointLight>,
}

/// Everything the depth, scene, and transparency passes draw, laid out in the GPU buffers.
struct FrameDraws<'a> {
    /// The opaque queues: terrain, mesh batches, and transient opaque triangles.
    opaque: &'a OpaqueQueues,
    /// The back-to-front transparent queue.
    transparent: &'a [DrawItem],
    /// The pipeline target layout the opaque passes write.
    targets: SceneTargets,
    draw_sky: bool,
    /// Image-based lighting for PBR surfaces (the black default without it).
    image_lighting: &'a wgpu::BindGroup,
    /// The frame's passes and their attachments, and the views they resolve to.
    plan: &'a FramePlan,
    frame_targets: &'a FrameTargets<'a>,
}

/// The last submitted camera (unjittered), for history resets and reprojection.
struct PreviousCamera {
    eye: Vec3,
    focus: Vec3,
    projection: Mat4,
    view: Mat4,
    view_projection: Mat4,
    /// Presentation seconds when it was drawn.
    time_seconds: f64,
}

/// Scene effects and the frame-to-frame state they need.
struct EffectsState {
    runtime: SceneEffectsRuntime,
    /// The motion target the scene pass writes, when temporal antialiasing or motion blur reads it.
    attachments: Option<SceneAttachments>,
    previous_models: HashMap<MeshInstanceId, Mat4>,
    previous_camera: Option<PreviousCamera>,
}

impl EffectsState {
    fn reset_history(&mut self) {
        self.runtime.reset_history();
        self.previous_models.clear();
        self.previous_camera = None;
    }

    /// Allocate the scene-pass attachment the effects read for a `size` frame (or release it), and return the targets
    /// the scene pass writes.
    fn prepare_attachments(&mut self, device: &wgpu::Device, size: (u32, u32)) -> SceneTargets {
        if !self.runtime.needs_motion_vectors() {
            self.attachments = None;
            return SceneTargets::Color;
        }
        if self.attachments.as_ref().is_none_or(|attachments| attachments.size() != size) {
            self.attachments = Some(SceneAttachments::new(device, size));
        }
        SceneTargets::ColorAndAttachments
    }
}

/// Everything the frame uniform block is packed from.
struct FrameUniformInputs<'a> {
    lighting: &'a FrameLighting,
    clusters: &'a LightClusters,
    cameras: &'a FrameCameras,
    /// The view depth volumetric fog covers, while it renders.
    fog_extent: Option<f32>,
    render_size: (u32, u32),
    /// The temporal antialiasing frame index, while it accumulates frames.
    temporal_noise: Option<u32>,
}

/// GPU copy of one asset, released when the asset is dropped everywhere.
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    asset: Weak<dyn std::any::Any + Send + Sync>,
}

/// Lists a frame fills and the next frame reuses, so preparing a frame allocates little.
#[derive(Default)]
struct FrameScratch {
    /// Instance records: identity (for transient triangles), opaque batches, blended meshes in queue order, then each
    /// rendering shadow view's casters.
    raws: Vec<InstanceRaw>,
    /// Transient vertices: opaque triangles, then the transparent queue's colored triangles.
    transient: Vec<MeshVertex>,
    camera: CameraBatches,
    blend_depths: Vec<f32>,
    /// The slots every rendering shadow view draws, each once.
    caster_slots: Vec<u32>,
}

/// The renderer's GPU resources and configuration, and the pass encoders that draw with them. Split from
/// [`Renderer`] so a frame can borrow them beside the effects state, profiler, batcher, and shadow planner.
struct FrameResources {
    // Pipelines and per-frame buffers.
    pipelines: ScenePipelines,
    post: PostProcessor,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    // The fog volume the frame group binds: volumetric fog's integrated volume, or a one-texel stand-in without it.
    frame_fog: wgpu::TextureView,
    fog_sampler: wgpu::Sampler,
    no_fog_view: wgpu::TextureView,
    _no_fog_texture: wgpu::Texture,
    atmosphere_buffer: wgpu::Buffer,
    atmosphere_bind_group: wgpu::BindGroup,
    // The frame's point lights and light clusters, bound in the frame group.
    lights: LightTextures,
    // The G-buffer, while deferred lighting runs.
    gbuffer: Option<GBuffer>,
    shadows: ShadowMaps,
    // Shading models, their materials' GPU copies (with generated mips), and the materials transient triangles draw
    // with. Image-based lighting's default group is black with zero intensity.
    models: ShadingModels,
    mipmaps: MipmapGenerator,
    materials: MaterialCache,
    default_lit: MaterialAsset,
    default_unlit: MaterialAsset,
    image_lighting: ImageLightingResources,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    transient_buffer: wgpu::Buffer,
    transient_capacity: usize,
    // Instanced particles, their images, and their instance buffer (FrameInputs::particles).
    particles: ParticlePass,
    // Resident terrain adapters for the scene's terrain instances.
    terrain: SceneTerrain,

    // Uploaded mesh copies keyed by asset identity.
    meshes: HashMap<u64, GpuMesh>,

    // Configuration, drawing size, debug view, and the shell's presentation clock.
    options: RendererOptions,
    debug_view: DebugView,
    physical_size: (u32, u32),
    scale_factor: f32,
    presentation_time: PresentationTime,

    // The shared device; resources above keep it alive internally, so field order is not load-bearing here.
    gpu: GpuContext,
    output_format: wgpu::TextureFormat,
}

/// The engine's renderer. Borrows scenes and cameras per frame; owns all GPU resources it creates.
pub struct Renderer {
    resources: FrameResources,
    // Scene effects, when enabled and supported.
    effects: Option<EffectsState>,
    // Per-phase frame timings, while profiling is on.
    profiler: FrameProfiler,
    // Frame preparation: scene batching, the shadow planner with its map cache, and reused lists.
    batcher: SceneBatcher,
    shadow_planner: ShadowPlanner,
    scratch: FrameScratch,
    diagnostics: RendererDiagnostics,
    // Buffer Overview: whether frames draw as the buffer grid, and its resources once first used.
    overview_enabled: bool,
    overview: Option<BufferOverview>,
}

impl Renderer {
    /// Create a renderer that presents into views of `output_format`.
    ///
    /// Lighting and blending happen in linear light and the presentation pass delivers sRGB-encoded display values in
    /// either kind of format: a non-sRGB view stores them as written, and an sRGB view receives decoded values that its
    /// hardware encoding turns back into the same bytes. The output starts as standard dynamic range; hosts presenting
    /// to an HDR surface call [`Renderer::set_display_output`].
    pub fn new(gpu: &GpuContext, output_format: wgpu::TextureFormat, options: RendererOptions) -> Self {
        let device = gpu.device();
        let capabilities = gpu.capabilities();
        let bias = options.shadows.map_or(
            CasterBias {
                constant: 0,
                slope_scale: 0.0,
            },
            |shadows| CasterBias {
                constant: shadows.depth_bias,
                slope_scale: shadows.depth_bias_slope_scale,
            },
        );
        let pipelines = ScenePipelines::new(device, capabilities.scene_format, bias, capabilities.compute);
        let mut mipmaps = MipmapGenerator::new(device);
        // Particles keep the scene pass's frame and shadow groups bound, then bind their sprite as group 2.
        let particles = ParticlePass::new(
            gpu,
            &mut mipmaps,
            &[&pipelines.frame_layout, &pipelines.shadow_layout],
            &scene_common(device),
            capabilities.scene_format,
        );
        let post = PostProcessor::new(
            device,
            gpu.queue(),
            capabilities.scene_format,
            output_format,
            options.post_process.clone(),
        );

        let frame_buffer = uniform_buffer(device, "frame uniforms", std::mem::size_of::<FrameUniforms>());
        let no_fog_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("no fog"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: FOG_VOLUME_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let no_fog_view = no_fog_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let fog_sampler = linear_sampler(device, "fog volume");
        let lights = LightTextures::new(device);
        let frame_bind_group = frame_group(
            device,
            &pipelines.frame_layout,
            &FrameBindings {
                buffer: &frame_buffer,
                fog: &no_fog_view,
                fog_sampler: &fog_sampler,
                lights: &lights,
            },
        );
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

        let models = ShadingModels::new(device);
        let materials = MaterialCache::new(device, gpu.queue());
        let image_lighting = ImageLightingResources::new(gpu, &pipelines.image_lighting_layout);

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
            display: DisplayOutput::STANDARD,
            ..Default::default()
        };

        let resources = FrameResources {
            pipelines,
            post,
            frame_buffer,
            frame_bind_group,
            frame_fog: no_fog_view.clone(),
            fog_sampler,
            no_fog_view,
            _no_fog_texture: no_fog_texture,
            atmosphere_buffer,
            atmosphere_bind_group,
            lights,
            gbuffer: None,
            shadows,
            models,
            mipmaps,
            materials,
            default_lit: MaterialAsset::default(),
            default_unlit: MaterialAsset::new(joy_render_types::Material::unlit(LinearColor::WHITE))
                .expect("opaque white unlit shading is a valid material"),
            image_lighting,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            transient_buffer,
            transient_capacity: INITIAL_TRANSIENT_CAPACITY,
            particles,
            terrain: SceneTerrain::new(bias),
            meshes: HashMap::new(),
            options: RendererOptions {
                render_scale: sanitized_render_scale(options.render_scale),
                ..options.clone()
            },
            debug_view: DebugView::Lit,
            physical_size: (0, 0),
            scale_factor: 1.0,
            presentation_time: PresentationTime::default(),
            gpu: gpu.clone(),
            output_format,
        };
        let mut renderer = Self {
            resources,
            effects: None,
            profiler: FrameProfiler::default(),
            batcher: SceneBatcher::default(),
            shadow_planner: ShadowPlanner::default(),
            scratch: FrameScratch::default(),
            diagnostics,
            overview_enabled: false,
            overview: None,
        };
        if let Err(error) = renderer.set_scene_effects(options.scene_effects) {
            log::error!("Scene effects are disabled: {error}");
        }
        renderer
    }

    /// Replace the scene effects; `None` or an all-off configuration disables them. Validation failures keep the
    /// current effects. Replacing effects restarts their history and the adapted exposure.
    pub fn set_scene_effects(&mut self, options: Option<SceneEffectsOptions>) -> Result<(), SceneEffectsError> {
        if let Some(options) = &options {
            options.validate()?;
        }
        let resources = &mut self.resources;
        resources.options.scene_effects = options;
        resources.post.reset_exposure(resources.gpu.queue());
        let setup = EffectsSetup {
            scene_format: resources.gpu.capabilities().scene_format,
            support: resources.effect_support(),
            frame_buffer: &resources.frame_buffer,
            lights: &resources.lights,
            shadow_layout: &resources.pipelines.shadow_layout,
        };
        self.effects = match options {
            Some(options) if options.any_enabled() => Some(EffectsState {
                runtime: SceneEffectsRuntime::new(resources.gpu.device(), options, &setup),
                attachments: None,
                previous_models: HashMap::new(),
                previous_camera: None,
            }),
            _ => None,
        };
        Ok(())
    }

    /// Register a game's shading model so materials can name it ([`Shading::Custom`](joy_render_types::Shading)).
    ///
    /// The model's WGSL is composed with the engine's scene contract and validated before anything compiles it, so a
    /// malformed model reports an error instead of failing inside the GPU device. A model that implements
    /// `materialEncode` and `materialDecode` shades deferred where the device supports it. Fails for a duplicate or
    /// empty name, WGSL that does not validate, a material group that does not match the definition, or deferred
    /// functions that read the material group. Materials naming a model before it registers are skipped (see
    /// [`RendererDiagnostics::unavailable_materials`]); the next frame renders every shadow map again, so casters that
    /// waited on the model appear in them.
    pub fn register_shading_model(&mut self, definition: ShadingModelDefinition) -> Result<(), RenderError> {
        let resources = &mut self.resources;
        let common = crate::pipelines::scene_common(resources.gpu.device());
        resources.models.register(resources.gpu.device(), definition, &common)?;
        self.shadow_planner.invalidate();
        Ok(())
    }

    /// Prefilter an equirectangular environment into image-based lighting for layered surfaces: a GGX specular cube
    /// and spherical-harmonic diffuse irradiance.
    ///
    /// Submits GPU work; call during setup or when the environment changes, then pass the map each frame through
    /// [`FrameInputs::image_lighting`]. Fails for malformed or oversized pixels.
    pub fn prepare_environment_map(
        &self,
        pixels: &EnvironmentPixels<'_>,
        options: &EnvironmentMapOptions,
    ) -> Result<EnvironmentMap, RenderError> {
        let resources = &self.resources;
        resources.image_lighting.prepare(&resources.gpu, &resources.pipelines.image_lighting_layout, pixels, options)
    }

    /// The configured scene effects.
    pub fn scene_effects(&self) -> Option<&SceneEffectsOptions> {
        self.resources.options.scene_effects.as_ref()
    }

    /// Which configured effects run on this device.
    pub fn scene_effects_capabilities(&self) -> SceneEffectsCapabilities {
        let options = self.resources.options.scene_effects.unwrap_or_default();
        capabilities(&options, self.resources.effect_support())
    }

    /// Selection and draw statistics of a scene terrain instance from the most recent frame; `None` when that frame did
    /// not prepare it (it was hidden, or outside the view without casting shadows).
    pub fn terrain_stats(&self, id: TerrainInstanceId) -> Option<&TerrainStats> {
        self.resources.terrain.stats(id)
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
        self.resources.physical_size = (physical_width, physical_height);
        self.resources.scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 { scale_factor } else { 1.0 };
    }

    /// Set the presentation clock for the next frame. The shell calls this before every render phase; renderers
    /// driven without a shell (tests and offline tools) advance it themselves, or temporal effects hold still.
    pub fn set_presentation_time(&mut self, time: PresentationTime) {
        self.resources.presentation_time = time;
    }

    /// The presentation clock the next frame renders with.
    pub fn presentation_time(&self) -> PresentationTime {
        self.resources.presentation_time
    }

    /// Drawing-buffer size in physical pixels.
    pub fn physical_size(&self) -> (u32, u32) {
        self.resources.physical_size
    }

    /// The fraction of the drawing-buffer resolution the scene renders at ([`RendererOptions::render_scale`]).
    pub fn render_scale(&self) -> f32 {
        self.resources.options.render_scale
    }

    /// Render the scene at `scale` times the drawing-buffer resolution from the next frame, clamped to
    /// [`MIN_RENDER_SCALE`]..=[`MAX_RENDER_SCALE`] (non-finite values mean full resolution). Lowering the scale trades
    /// sharpness for scene, effect, and bloom work, which falls with the pixel count; a change restarts temporal effect
    /// history and reallocates the scene targets on the next frame.
    pub fn set_render_scale(&mut self, scale: f32) {
        let scale = sanitized_render_scale(scale);
        if scale != self.resources.options.render_scale {
            self.resources.options.render_scale = scale;
            self.reset_history();
        }
    }

    /// Size in physical pixels the scene renders at: each axis of the drawing buffer times the render scale, rounded,
    /// and at least one pixel while the drawing buffer has area. Screen-space queries ([`Renderer::screen_ray`],
    /// [`Renderer::project`]) stay in logical pixels and do not depend on it.
    pub fn render_size(&self) -> (u32, u32) {
        self.resources.render_size()
    }

    /// The presentation target's encoding and HDR headroom.
    pub fn display_output(&self) -> DisplayOutput {
        self.resources.post.display_output()
    }

    /// Write for `display` from the next frame. The shell sets it when it configures the surface and whenever the
    /// display's headroom changes, so it must describe the views passed to [`Renderer::encode`]. Standard output (the
    /// default) clamps to SDR white; extended output keeps SDR white at one and tone maps highlights into the headroom.
    pub fn set_display_output(&mut self, display: DisplayOutput) {
        self.resources.post.set_display_output(display);
        self.diagnostics.display = self.resources.post.display_output();
    }

    /// Viewport size in logical (CSS) pixels.
    pub fn logical_size(&self) -> Vec2 {
        self.resources.logical_size()
    }

    /// Current post-processing settings.
    pub fn post_process(&self) -> &PostProcessSettings {
        self.resources.post.settings()
    }

    /// Replace post-processing settings for the next frame. Cheap enough to call every frame (for example with
    /// settings resolved from post-process volumes at the camera); only changes to the effect graph's shape (bloom,
    /// blur, or lens streaks turning on or off, or a new bloom quality) reallocate targets.
    pub fn set_post_process(&mut self, settings: PostProcessSettings) {
        if self.resources.options.post_process != settings {
            self.resources.options.post_process = settings.clone();
            self.resources.post.set_settings(settings);
        }
    }

    /// Whether the procedural sky draws behind scenes that supply an environment ([`RendererOptions::atmosphere`]).
    pub fn atmosphere(&self) -> bool {
        self.resources.options.atmosphere
    }

    /// Draw (or stop drawing) the procedural sky from the next frame. Tools that light a studio preview with their own
    /// key light over a plain background turn it off; hosts that share one renderer between tools re-apply it when a
    /// tool becomes active.
    pub fn set_atmosphere(&mut self, enabled: bool) {
        self.resources.options.atmosphere = enabled;
    }

    /// The current shadow settings; `None` when shadows are off.
    pub fn shadow_options(&self) -> Option<ShadowOptions> {
        self.resources.options.shadows
    }

    /// Replace shadow settings from the next frame; `None` disables shadow work.
    ///
    /// Distances and the receiver normal bias apply immediately, so a preview can fit them to its camera every frame.
    /// Changing the quality or map sizes (or turning shadows on or off) reallocates the shadow maps, and the next frame
    /// renders every map again. The caster raster bias (`depth_bias` and `depth_bias_slope_scale`) is compiled into the
    /// caster pipelines and stays at the values the renderer was created with.
    pub fn set_shadow_options(&mut self, options: Option<ShadowOptions>) {
        let resources = &mut self.resources;
        let max_texture_dimension = resources.gpu.capabilities().max_texture_dimension;
        let layout = |options: Option<ShadowOptions>| {
            options.map(|options| {
                (
                    options.quality,
                    options.directional_map_size(max_texture_dimension),
                    options.point_atlas_size(max_texture_dimension),
                )
            })
        };
        if layout(options) != layout(resources.options.shadows) {
            resources.shadows = ShadowMaps::new(
                resources.gpu.device(),
                &resources.pipelines.shadow_layout,
                &resources.pipelines.caster_layout,
                options,
                max_texture_dimension,
            );
            self.shadow_planner.invalidate();
        }
        resources.options.shadows = options;
    }

    /// Whether opaque surfaces shade with deferred lighting where the device supports it.
    pub fn deferred(&self) -> bool {
        self.resources.options.deferred
    }

    /// Prefer deferred lighting (`true`) or forward shading for later frames. Devices without deferred support (WebGL 2)
    /// shade forward either way; see [`deferred_supported`](Self::deferred_supported).
    pub fn set_deferred(&mut self, enabled: bool) {
        self.resources.options.deferred = enabled;
        if !enabled {
            self.resources.gbuffer = None;
        }
    }

    /// Whether this device can run deferred lighting.
    pub fn deferred_supported(&self) -> bool {
        self.resources.gpu.capabilities().deferred
    }

    /// Compile, ahead of the first frames, the pipelines frames draw with for every registered shading model under the
    /// current options: its forward (or, when deferred lighting runs, G-buffer and lighting) pass with the scene targets
    /// the current effects need, its blended pass, and its masked depth and caster passes.
    ///
    /// Pipelines otherwise compile when a frame first needs them, which stalls that frame (noticeably in browsers). The
    /// shells call this once after `App::start`; call it again after registering shading models or changing effects or
    /// the depth pre-pass. Frames still compile anything this did not anticipate.
    pub fn warm_up_pipelines(&mut self) {
        let targets = match &self.effects {
            Some(state) if state.runtime.needs_motion_vectors() => SceneTargets::ColorAndAttachments,
            _ => SceneTargets::Color,
        };
        let resources = &mut self.resources;
        let device = resources.gpu.device().clone();
        let deferred = resources.deferred_active();
        for model in 0..resources.models.len() {
            let mut passes = vec![ModelPass::Scene(targets), ModelPass::Blend, ModelPass::MaskedCaster];
            if resources.options.depth_prepass {
                passes.push(ModelPass::MaskedDepth);
            }
            if deferred && resources.models.get(model).deferred {
                passes.extend([ModelPass::GBuffer(targets), ModelPass::Lighting]);
            }
            for pass in passes {
                resources.pipelines.ensure_model(&device, &resources.models, model, pass);
            }
        }
        resources.pipelines.ensure_atmosphere(&device, targets);
    }

    /// Whether frames record a [`FrameProfile`].
    pub fn profiling(&self) -> bool {
        self.profiler.enabled()
    }

    /// Record how long each phase of every frame takes: CPU recording time always, and GPU time where the device has
    /// timestamp queries ([`GpuCapabilities::timestamps`](crate::GpuCapabilities::timestamps); not WebGL 2). Off by
    /// default; profiling adds a timestamp per phase and a small readback, and nothing while off.
    pub fn set_profiling(&mut self, enabled: bool) {
        let gpu = &self.resources.gpu;
        let timestamps = gpu.capabilities().timestamps;
        self.profiler.set_enabled(gpu.device(), gpu.queue(), enabled, timestamps);
    }

    /// The latest complete frame profile while profiling. GPU timings are read back without stalling, so they
    /// describe a frame a few frames old ([`FrameProfile::frame`]); hosts must submit each encoded frame for them to
    /// arrive.
    pub fn frame_profile(&self) -> Option<&FrameProfile> {
        self.profiler.latest()
    }

    /// What mesh surfaces draw as: the lit scene, or one surface attribute.
    pub fn debug_view(&self) -> DebugView {
        self.resources.debug_view
    }

    /// Whether frames draw as the Buffer Overview grid ([`BUFFER_OVERVIEW_TILES`](crate::BUFFER_OVERVIEW_TILES)).
    pub fn buffer_overview(&self) -> bool {
        self.overview_enabled
    }

    /// Draw every frame as the Buffer Overview grid from the next frame, or return to the single view. The overview
    /// redraws the frame once per tile, so it is for development inspection only.
    pub fn set_buffer_overview(&mut self, enabled: bool) {
        self.overview_enabled = enabled;
        if !enabled {
            self.overview = None;
        }
    }

    /// Draw mesh and terrain surfaces as `view` from the next frame ([`DebugView::Lit`] restores the lit scene).
    pub fn set_debug_view(&mut self, view: DebugView) {
        self.resources.debug_view = view;
    }

    /// Statistics from the most recent frame.
    pub fn diagnostics(&self) -> &RendererDiagnostics {
        &self.diagnostics
    }

    /// The device this renderer draws with.
    pub fn gpu(&self) -> &GpuContext {
        &self.resources.gpu
    }

    /// Format of the views this renderer presents into; other renderers drawing into the same target must match it.
    pub fn output_format(&self) -> wgpu::TextureFormat {
        self.resources.output_format
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
        let mut encoder = self.resources.gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render frame"),
        });
        self.encode(&mut encoder, scene, camera, inputs, output)?;
        self.resources.gpu.queue().submit([encoder.finish()]);
        Ok(())
    }

    /// Record one frame into `encoder` without submitting, so a host can append overlays (such as development UI).
    ///
    /// Uploads happen immediately through the queue. Fails without recording anything for a zero-sized drawing buffer,
    /// invalid ambient or image lighting, invalid terrain settings, or a texture larger than the device supports.
    /// Invalid lights are skipped rather than failing the frame (see [`FrameInputs`]).
    pub fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &RenderScene,
        camera: &impl Camera,
        inputs: &FrameInputs<'_>,
        output: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        let (width, height) = self.resources.physical_size;
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidFrame("the drawing buffer has no area; call resize first".into()));
        }
        validate_frame(inputs)?;

        let camera_frame = self.camera_frame(camera);
        if self.overview_enabled {
            // The overview submits its own work per tile, ahead of whatever the host appends to `encoder`.
            return self.encode_buffer_overview(scene, inputs, output, camera_frame);
        }
        self.profiler.begin_frame(self.resources.gpu.device());
        self.profiler.mark(encoder, FramePhase::Prepare);
        let result = self.encode_frame(encoder, scene, inputs, output, camera_frame);
        match &result {
            Ok(()) => self.profiler.end_frame(encoder),
            // The planner already counted this frame's shadow maps as rendered; render them all on the next frame.
            Err(_) => self.shadow_planner.invalidate(),
        }
        result
    }

    /// Draw the Buffer Overview into `output`: the lit frame (with scene effects) and its depth and motion targets
    /// first, then every other tile's debug view with scene effects suspended, each submitted before the next renders
    /// into the shared offscreen frame. Restores the debug view and effects afterwards, even on failure.
    fn encode_buffer_overview(
        &mut self,
        scene: &RenderScene,
        inputs: &FrameInputs<'_>,
        output: &wgpu::TextureView,
        camera_frame: CameraFrame,
    ) -> Result<(), RenderError> {
        let device = self.resources.gpu.device().clone();
        let queue = self.resources.gpu.queue().clone();
        let size = self.resources.physical_size;
        let gap = (BUFFER_OVERVIEW_GAP * self.resources.scale_factor).round();
        let near_far = (camera_frame.near, camera_frame.far);
        let mut overview =
            self.overview.take().unwrap_or_else(|| BufferOverview::new(&device, self.resources.output_format, size));
        let frame_view = overview.frame(&device, size).clone();
        let saved_view = self.resources.debug_view;

        // The lit frame, profiled like an ordinary frame, then its own targets into the color, depth, and motion tiles.
        self.resources.debug_view = DebugView::Lit;
        let mut lit = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("buffer overview lit frame"),
        });
        self.profiler.begin_frame(&device);
        self.profiler.mark(&mut lit, FramePhase::Prepare);
        if let Err(error) = self.encode_frame(&mut lit, scene, inputs, &frame_view, camera_frame) {
            self.shadow_planner.invalidate();
            self.resources.debug_view = saved_view;
            self.overview = Some(overview);
            return Err(error);
        }
        self.profiler.end_frame(&mut lit);
        let depth = self.resources.post.depth_sample_view().clone();
        let motion = self
            .effects
            .as_ref()
            .and_then(|state| state.attachments.as_ref())
            .map(|attachments| attachments.motion().clone());
        for (index, tile) in BUFFER_OVERVIEW_TILES.iter().copied().enumerate() {
            if matches!(tile, OverviewTile::View(DebugView::Lit) | OverviewTile::Depth | OverviewTile::Motion) {
                overview.draw_tile(
                    &device,
                    &queue,
                    &mut lit,
                    tile,
                    index,
                    output,
                    size,
                    gap,
                    &depth,
                    motion.as_ref(),
                    near_far,
                    index == 0,
                );
            }
        }
        queue.submit([lit.finish()]);

        // Every other view, with effects set aside so temporal history and exposure see only the lit frames.
        let effects = self.effects.take();
        let mut result = Ok(());
        for (index, tile) in BUFFER_OVERVIEW_TILES.iter().copied().enumerate() {
            let OverviewTile::View(view) = tile else {
                continue;
            };
            if view == DebugView::Lit {
                continue;
            }
            self.resources.debug_view = view;
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("buffer overview tile"),
            });
            if let Err(error) = self.encode_frame(&mut encoder, scene, inputs, &frame_view, camera_frame) {
                self.shadow_planner.invalidate();
                result = Err(error);
                break;
            }
            let depth = self.resources.post.depth_sample_view().clone();
            overview.draw_tile(
                &device,
                &queue,
                &mut encoder,
                tile,
                index,
                output,
                size,
                gap,
                &depth,
                None,
                near_far,
                false,
            );
            queue.submit([encoder.finish()]);
        }
        self.effects = effects;
        self.resources.debug_view = saved_view;
        self.overview = Some(overview);
        result
    }

    /// Record one validated frame; see the module documentation for the pass order.
    fn encode_frame(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &RenderScene,
        inputs: &FrameInputs<'_>,
        output: &wgpu::TextureView,
        camera_frame: CameraFrame,
    ) -> Result<(), RenderError> {
        // The frame borrows each owner on its own: the GPU resources, the effects, and the preparation state.
        let Self {
            resources,
            effects,
            profiler,
            batcher,
            shadow_planner,
            scratch,
            diagnostics,
            ..
        } = self;
        let mut effects = effects.as_mut();
        let device = resources.gpu.device().clone();
        let queue = resources.gpu.queue().clone();

        // The scene and its effects draw at the render size; the post chain upscales into the output size.
        let output_size = resources.physical_size;
        let render_size = resources.render_size();

        // Cameras. Effects may jitter the raster projection; everything that decides what to draw uses the stable one.
        let cameras = match effects.as_deref_mut() {
            Some(state) => resources.prepare_effects_camera(state, &camera_frame),
            None => FrameCameras {
                stable: camera_frame,
                raster: camera_frame,
                previous_view_projection: camera_frame.view_projection(),
                previous_view: camera_frame.view,
            },
        };
        let logical_height = output_size.1 as f32 / resources.scale_factor;
        let terrain =
            resources.terrain.prepare(&resources.gpu, &resources.pipelines, scene, &camera_frame, logical_height)?;

        // Lights: the strongest point lights that can reach the view, and the key directional light when usable.
        let selected = select_point_lights(inputs.lights, &camera_frame);
        let lighting = FrameLighting {
            key: inputs.key_light().filter(DirectionalLight::is_valid),
            skylight: inputs.skylight(),
            ambient: inputs.resolved_ambient(),
            points: selected.lights,
        };

        // Render proxies, refreshed for the edits since the last frame, and the instances the camera sees. Every mesh
        // and material they draw gets a GPU copy before anything is laid out.
        let proxies = scene.proxies();
        let view_depth = ViewDepth::new(&camera_frame);
        let culled_meshes = batcher.cull_camera(&proxies, scene, camera_frame.view_projection());
        resources.prepare_default_materials()?;
        resources.upload_assets(instance_assets(scene, batcher.visible().iter().copied()))?;

        // Camera batches. Instances: identity (for transient triangles), opaque batches, blended meshes in queue order,
        // then each rendering shadow view's casters.
        let raws = &mut scratch.raws;
        raws.clear();
        raws.push(InstanceRaw::IDENTITY);
        let camera_inputs = CameraFrameInputs {
            scene,
            proxies: &proxies,
            materials: &resources.materials,
            view_depth,
            previous_models: effects.as_deref().map(|state| &state.previous_models),
        };
        batcher.batch_camera(camera_inputs, raws, &mut scratch.camera);
        let camera_batches = &scratch.camera;

        // Transient opaque triangles lead the transient vertex stream; the transparent queue appends its colored
        // triangles after them.
        let transient = &mut scratch.transient;
        transient.clear();
        append_opaque_transient(inputs.opaque_triangles, transient);
        let transient_opaque = transient.len() as u32;

        // Particle integration (1 of 2): particles cull and sort for the stable camera, and their images upload before
        // any pass opens, like meshes and materials; each surviving batch then joins the transparent queue at one depth
        // and becomes a particle draw item.
        let particle_view = ParticleView {
            view_projection: camera_frame.view_projection(),
            eye: view_depth.eye,
            forward: view_depth.forward,
        };
        let particle_counts = resources.particles.prepare(
            &resources.gpu,
            &mut resources.mipmaps,
            encoder,
            inputs.particles,
            &particle_view,
        )?;
        scratch.blend_depths.clear();
        scratch.blend_depths.extend(camera_batches.blends.iter().map(|blend| blend.depth));
        let transparent = build_transparent_queue(
            &scratch.blend_depths,
            inputs.transparent_triangles,
            resources.particles.batches().iter().map(|batch| batch.depth),
            view_depth.eye,
            view_depth.forward,
            transient,
        );
        let blend_start = raws.len() as u32;
        raws.extend(transparent.mesh_order.iter().map(|index| camera_batches.blends[*index].raw));
        let transparent_items = resources.transparent_items(&transparent, &camera_batches.blends, blend_start);

        // Shadows: which maps render, and each rendering view's casters, batched once their materials are prepared.
        let opaque_triangles = &inputs.opaque_triangles[..transient_opaque as usize];
        let transient_bounds = triangle_bounds(opaque_triangles);
        let shadow_frame = ShadowFrame {
            options: resources.options.shadows,
            max_texture_dimension: resources.gpu.capabilities().max_texture_dimension,
            key: lighting.key,
            points: &lighting.points,
            camera: &camera_frame,
            render_size,
            casters: FrameCasters {
                proxies: &proxies,
                transient: transient_bounds,
                terrain: &terrain.caster_bounds,
            },
            changes: ShadowInputs {
                scene: scene.id(),
                proxies: &proxies,
                transient: opaque_triangles,
                transient_bounds,
                terrain: TerrainShadowKey {
                    revision: scene.terrain_revision(),
                    eye: camera_frame.eye,
                    focus: camera_frame.focus,
                    projection: camera_frame.projection,
                    logical_height,
                },
                terrain_bounds: &terrain.caster_bounds,
            },
        };
        let mut shadows = shadow_planner.plan(&shadow_frame, &mut resources.shadows, &device, &queue);
        // Views share most casters (cascades nest), so each caster uploads once.
        scratch.caster_slots.clear();
        scratch.caster_slots.extend(shadows.views.iter().flat_map(|view| view.casters.iter().copied()));
        scratch.caster_slots.sort_unstable();
        scratch.caster_slots.dedup();
        resources.upload_assets(instance_assets(scene, scratch.caster_slots.iter().copied()))?;
        let caster_inputs = CasterFrameInputs {
            scene,
            proxies: &proxies,
            materials: &resources.materials,
        };
        let mut view_casters = Vec::with_capacity(shadows.views.len());
        for view in &mut shadows.views {
            view_casters.push(batcher.batch_casters(&caster_inputs, &view.casters, raws, &mut view.items));
            if view.transient && transient_opaque > 0 {
                view.items.push(resources.transient_item(0..transient_opaque));
            }
            view.items.push(terrain_item());
        }
        let caster_instances = shadow_planner.record_casters(&shadows, &view_casters);

        // Terrain selects its tiles once, then keeps the ones each view (the camera and every rendering shadow view)
        // reaches.
        let terrain_shadows: Vec<TerrainShadowView> = shadows
            .views
            .iter()
            .map(|view| TerrainShadowView {
                view_projection: view.matrix,
                texel: view.texel,
            })
            .collect();
        resources.terrain.prepare_draws(&resources.gpu, camera_frame.view_projection(), &terrain_shadows)?;

        // The opaque queue: terrain first, as the largest occluder, then mesh batches front to back, then transient
        // opaque triangles.
        let mut depth_order = Vec::with_capacity(camera_batches.opaque.len() + 2);
        if resources.terrain.has_visible() {
            depth_order.push(terrain_item());
        }
        depth_order.extend(camera_batches.opaque.iter().cloned());
        if transient_opaque > 0 {
            depth_order.push(resources.transient_item(0..transient_opaque));
        }
        let opaque = OpaqueQueues::new(depth_order, resources.options.depth_prepass);
        resources.write_instances(raws);
        resources.write_transient(transient);

        // Light clusters for the stable camera, carrying each light's shadow slot.
        let clusters = LightClusters::build(&lighting.points, &shadows.light_slots, &camera_frame);
        resources.lights.upload(&queue, &clusters);

        // Volumetric fog sizes its volumes for this frame before the frame group binds the integrated one.
        if let Some(state) = effects.as_deref_mut() {
            state.runtime.prepare(&device, render_size);
        }
        let fog = effects.as_deref().and_then(|state| state.runtime.fog());
        resources.bind_frame_fog(fog.map(|(view, _)| view));
        let fog_extent = fog.map(|(_, extent)| extent);

        // Frame, sky, and image-lighting uniforms.
        let temporal_noise =
            effects.as_deref().filter(|state| state.runtime.jitters()).map(|state| state.runtime.frame_index());
        let frame = FrameUniformInputs {
            lighting: &lighting,
            clusters: &clusters,
            cameras: &cameras,
            fog_extent,
            render_size,
            temporal_noise,
        };
        resources.write_frame_uniforms(&frame);
        resources.image_lighting.write_parameters(&queue, inputs.image_lighting.as_ref());
        let draw_sky = resources.options.atmosphere && inputs.environment.is_some();
        if draw_sky {
            resources.write_atmosphere_uniforms(inputs, &cameras.raster);
        }

        // Scene targets: with effects that read motion, opaque materials write it beside their color. Transparency joins
        // the last opaque pass unless fog must apply between opaque surfaces and transparency, or the pass writes
        // attachments that blended pipelines do not. Deferred lighting runs only when some surface can use it, so
        // scenes of unlit or forward-only materials skip the G-buffer entirely.
        let targets = match effects.as_deref_mut() {
            Some(state) => state.prepare_attachments(&device, render_size),
            None => SceneTargets::Color,
        };
        let deferred = resources.deferred_active() && opaque.has_deferred();
        resources.ensure_frame_pipelines(&device, targets, &opaque, &transparent_items, &shadows.views, deferred);
        resources.terrain.ensure_lit(&device, targets, deferred);
        let lighting_effects = effects.as_deref().is_some_and(|state| state.runtime.has_lighting_effects());
        // Soft particles read the opaque depth, which a pass can only sample while it attaches the depth read-only, so
        // they also move transparency into its own pass.
        let soft_particles = resources.particles.reads_scene_depth();
        let transparency_in_scene = targets == SceneTargets::Color && !lighting_effects && !soft_particles;
        resources.post.prepare(&device, &queue, render_size, output_size);
        resources.particles.bind_scene_depth(&device, soft_particles.then(|| resources.post.depth_sample_view()));
        if deferred && resources.gbuffer.as_ref().is_none_or(|gbuffer| gbuffer.size() != render_size) {
            resources.gbuffer = Some(GBuffer::new(&device, render_size));
        }

        // The frame's passes, with every attachment's operations derived from their order.
        let plan = FramePlan::new(&FrameShape {
            cascades: shadows.views.iter().filter_map(|view| cascade_index(view.target)).collect(),
            point_atlas: shadows.renders_point_faces().then_some(if shadows.clear_atlas {
                Load::Clear
            } else {
                Load::Keep
            }),
            fog: lighting_effects,
            depth_prepass: resources.options.depth_prepass,
            deferred,
            motion: targets == SceneTargets::ColorAndAttachments,
            transparency_in_scene,
            soft_particles,
            composite: effects.is_some(),
        });
        let motion_view = effects
            .as_deref()
            .and_then(|state| state.attachments.as_ref())
            .map(|attachments| attachments.motion().clone());
        let frame_targets = FrameTargets {
            scene_color: resources.post.scene_view(),
            scene_depth: resources.post.depth_view(),
            motion: motion_view.as_ref(),
            gbuffer: resources.gbuffer.as_ref().filter(|_| deferred).map(GBuffer::view),
            cascades: resources.shadows.cascade_layers(),
            atlas: resources.shadows.atlas(),
            clear_color: inputs.clear_color,
        };
        let draws = FrameDraws {
            opaque: &opaque,
            transparent: &transparent_items,
            targets,
            draw_sky,
            image_lighting: inputs
                .image_lighting
                .map_or(resources.image_lighting.default_group(), |lighting| &lighting.map.bind_group),
            plan: &plan,
            frame_targets: &frame_targets,
        };

        // Shadow maps that render, then fog's volume, then the depth pre-pass.
        let effects_frame = effects.as_deref().map(|state| resources.effects_frame(state, &cameras, render_size));
        profiler.mark(encoder, FramePhase::Shadows);
        let mut draw_calls = resources.encode_shadow_views(encoder, &shadows, &draws);
        if let (Some(state), Some(frame)) = (effects.as_deref_mut(), &effects_frame)
            && state.runtime.has_lighting_effects()
        {
            profiler.mark(encoder, FramePhase::Volumetrics);
            state.runtime.encode_volume(&queue, encoder, frame, resources.shadows.receiver());
        }
        if let Some(planned) = plan.pass(FramePass::DepthPrepass) {
            profiler.mark(encoder, FramePhase::DepthPrepass);
            draw_calls += resources.encode_depth_prepass(encoder, planned, &draws);
        }

        // Opaque shading: deferred surfaces, their lighting, then whatever shades forward; or one forward pass.
        let mut triangles = 0;
        let mut lighting_passes = 0;
        if let Some(planned) = plan.pass(FramePass::GBuffer) {
            profiler.mark(encoder, FramePhase::GBuffer);
            let (calls, drawn) = resources.encode_gbuffer_pass(encoder, planned, &draws);
            draw_calls += calls;
            triangles += drawn;
        }
        if let Some(planned) = plan.pass(FramePass::DeferredLighting) {
            profiler.mark(encoder, FramePhase::Lighting);
            lighting_passes = resources.encode_lighting_passes(encoder, planned, &draws);
            draw_calls += lighting_passes;
        }
        profiler.mark(encoder, FramePhase::Scene);
        let scene_pass = plan.pass(FramePass::Scene).expect("every frame plans a scene pass");
        let (calls, drawn) = resources.encode_scene_pass(encoder, scene_pass, &draws, deferred, transparency_in_scene);
        draw_calls += calls;
        triangles += drawn;
        // Without scene effects, only soft particles move transparency out of the scene pass; it draws here instead.
        if effects.is_none() && !transparency_in_scene {
            profiler.mark(encoder, FramePhase::Transparency);
            let (calls, drawn) = resources.encode_transparency_pass(encoder, &draws);
            draw_calls += calls;
            triangles += drawn;
        }

        if let (Some(state), Some(frame)) = (effects, &effects_frame) {
            let (calls, drawn) = resources.encode_effects(encoder, state, frame, &draws, profiler);
            draw_calls += calls;
            triangles += drawn;
            state.previous_models.clear();
            state.previous_models.extend(camera_batches.models.iter().copied());
        }

        profiler.mark(encoder, FramePhase::Post);
        resources.post.encode(encoder, output);
        drop(proxies);
        resources.release_dropped_meshes();
        resources.particles.release_dropped();
        resources.materials.release_dropped();

        diagnostics.visible = camera_batches.visible + terrain.visible;
        diagnostics.culled = culled_meshes + terrain.culled;
        diagnostics.draw_calls = draw_calls;
        diagnostics.triangles = triangles;
        diagnostics.shadow_casters = caster_instances;
        diagnostics.shadows_active = shadows.cascades > 0;
        diagnostics.shadow_views = shadows.views_in_use;
        diagnostics.shadow_maps_rendered = shadows.views.len();
        diagnostics.shadow_point_lights = shadows.point_lights;
        diagnostics.point_lights = lighting.points.len();
        diagnostics.point_lights_omitted = selected.omitted;
        diagnostics.cluster_light_references = clusters.reference_count();
        diagnostics.cluster_references_dropped = clusters.dropped;
        diagnostics.deferred = deferred;
        diagnostics.lighting_passes = lighting_passes;
        diagnostics.forward_opaque_batches = if deferred { opaque.forward_batches() } else { 0 };
        diagnostics.render_size = render_size;
        diagnostics.unavailable_materials = camera_batches.unavailable;
        diagnostics.particles = particle_counts.instances;
        diagnostics.particle_batches_culled = particle_counts.culled_batches;
        Ok(())
    }
}

/// The pass a draw queue records into, which decides each item's pipeline.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DrawPass {
    /// A shadow view, by its index among the frame's rendering views, with its view-projection.
    Shadow { view: usize, matrix: Mat4 },
    /// The depth pre-pass.
    Depth,
    /// The G-buffer pass writing these targets.
    GBuffer(SceneTargets),
    /// A forward scene pass writing these targets.
    Scene(SceneTargets),
    /// The transparent queue.
    Transparent,
}

impl FrameResources {
    /// What this device offers scene effects.
    fn effect_support(&self) -> EffectSupport {
        let capabilities = self.gpu.capabilities();
        EffectSupport {
            compute: capabilities.compute,
            float_targets: capabilities.hdr,
        }
    }

    /// Size in physical pixels the scene renders at; see [`Renderer::render_size`].
    fn render_size(&self) -> (u32, u32) {
        scaled_render_size(self.physical_size, self.options.render_scale)
    }

    /// Viewport size in logical (CSS) pixels.
    fn logical_size(&self) -> Vec2 {
        Vec2::new(self.physical_size.0 as f32, self.physical_size.1 as f32) / self.scale_factor
    }

    /// Whether the next frame shades deferred: preferred, supported, and showing the lit scene (debug views shade
    /// forward, where every surface attribute is at hand).
    fn deferred_active(&self) -> bool {
        self.options.deferred && self.gpu.capabilities().deferred && self.debug_view == DebugView::Lit
    }

    // --- Draw items -----------------------------------------------------------------------------------------------

    /// The item that draws `vertices` of the transient stream as lit opaque triangles.
    fn transient_item(&self, vertices: std::ops::Range<u32>) -> DrawItem {
        DrawItem {
            geometry: DrawGeometry::Transient {
                vertices,
            },
            model: LIT_MODEL_ID,
            material: self.default_lit.id(),
            instances: 0..1,
            masked: false,
            deferred: true,
        }
    }

    /// The transparent queue as draw items: blended meshes at their instances (from `blend_start`, in queue order),
    /// unlit transient triangles, and particle batches.
    fn transparent_items(&self, queue: &TransparentQueue, blends: &[BlendDraw], blend_start: u32) -> Vec<DrawItem> {
        queue
            .steps
            .iter()
            .map(|step| match step {
                TransparentStep::Mesh(position) => {
                    let instance = blend_start + *position as u32;
                    DrawItem {
                        instances: instance..instance + 1,
                        ..blends[queue.mesh_order[*position]].item.clone()
                    }
                }
                TransparentStep::Colored(vertices) => DrawItem {
                    geometry: DrawGeometry::Transient {
                        vertices: vertices.clone(),
                    },
                    model: UNLIT_MODEL_ID,
                    material: self.default_unlit.id(),
                    instances: 0..1,
                    masked: false,
                    deferred: false,
                },
                // Particle integration: a prepared batch takes its place in the back-to-front order.
                TransparentStep::Particles(batch) => DrawItem {
                    geometry: DrawGeometry::Particles {
                        batch: *batch,
                    },
                    model: UNLIT_MODEL_ID,
                    material: OWN_MATERIAL,
                    instances: 0..0,
                    masked: false,
                    deferred: false,
                },
            })
            .collect()
    }

    /// Draw `items` in order into an open pass that already binds its groups; returns draw calls and triangles.
    fn draw_items<'item>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        items: impl IntoIterator<Item = &'item DrawItem>,
        draw_pass: DrawPass,
        image_lighting: Option<&wgpu::BindGroup>,
    ) -> (usize, usize) {
        let mut bound = BoundState::default();
        let (mut draw_calls, mut triangles) = (0, 0);
        for item in items {
            let (calls, drawn) = self.draw_item(pass, &mut bound, item, draw_pass, image_lighting);
            draw_calls += calls;
            triangles += drawn;
        }
        (draw_calls, triangles)
    }

    /// Draw one item; returns its draw calls and triangles.
    fn draw_item(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bound: &mut BoundState,
        item: &DrawItem,
        draw_pass: DrawPass,
        image_lighting: Option<&wgpu::BindGroup>,
    ) -> (usize, usize) {
        match &item.geometry {
            DrawGeometry::Mesh {
                asset,
                indices,
            } => {
                let Some(mesh) = self.meshes.get(asset) else {
                    return (0, 0);
                };
                self.bind_item(pass, bound, item, draw_pass, image_lighting);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(indices.clone(), 0, item.instances.clone());
                (1, item.triangles())
            }
            DrawGeometry::Transient {
                vertices,
            } => {
                self.bind_item(pass, bound, item, draw_pass, image_lighting);
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(vertices.clone(), item.instances.clone());
                (1, item.triangles())
            }
            DrawGeometry::Terrain => {
                // Terrain binds its own pipeline, vertex buffers, and material-slot group.
                *bound = BoundState::default();
                self.draw_terrain(pass, item, draw_pass)
            }
            DrawGeometry::Particles {
                batch,
            } => self.draw_particles(pass, bound, *batch),
        }
    }

    /// Bind what a mesh or transient item draws with in `draw_pass`: its model's pipeline (or the shared depth-only
    /// one), its material group, and the instance buffer.
    fn bind_item(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bound: &mut BoundState,
        item: &DrawItem,
        draw_pass: DrawPass,
        image_lighting: Option<&wgpu::BindGroup>,
    ) {
        if !bound.instances {
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            bound.instances = true;
        }
        match draw_pass {
            // Masked surfaces alpha-test through their shading model, so cut-out holes stay open for whatever lies
            // behind them; everything else shares one depth-only pipeline.
            DrawPass::Shadow {
                ..
            }
            | DrawPass::Depth => {
                let (masked_pass, shared, shared_pipeline) = if matches!(draw_pass, DrawPass::Depth) {
                    (ModelPass::MaskedDepth, BoundPipeline::Depth, &self.pipelines.depth)
                } else {
                    (ModelPass::MaskedCaster, BoundPipeline::Caster, &self.pipelines.caster)
                };
                if item.masked {
                    self.bind_model(pass, bound, item.model, masked_pass);
                    self.bind_material(pass, bound, item.material);
                } else if bound.pipeline != Some(shared) {
                    pass.set_pipeline(shared_pipeline);
                    bound.pipeline = Some(shared);
                }
            }
            DrawPass::GBuffer(targets) => {
                pass.set_stencil_reference(lighting_slot(item.model));
                self.bind_model(pass, bound, item.model, ModelPass::GBuffer(targets));
                self.bind_material(pass, bound, item.material);
            }
            DrawPass::Scene(targets) => {
                self.bind_model(pass, bound, item.model, ModelPass::Scene(targets));
                self.bind_material(pass, bound, item.material);
            }
            DrawPass::Transparent => {
                // Particle batches rebind group 2, so model draws bind image lighting again after one.
                if bound.environment_replaced
                    && let Some(group) = image_lighting
                {
                    pass.set_bind_group(2, group, &[]);
                    bound.environment_replaced = false;
                }
                self.bind_model(pass, bound, item.model, ModelPass::Blend);
                self.bind_material(pass, bound, item.material);
            }
        }
    }

    /// Draw the terrain a pass's view reaches with the terrain pipeline for that pass; returns draw calls and
    /// triangles.
    fn draw_terrain(&self, pass: &mut wgpu::RenderPass<'_>, item: &DrawItem, draw_pass: DrawPass) -> (usize, usize) {
        match draw_pass {
            DrawPass::Shadow {
                view,
                matrix,
            } => (self.terrain.draw_shadow(pass, view, matrix), 0),
            DrawPass::Depth => (self.terrain.draw_depth(pass), 0),
            DrawPass::GBuffer(targets) => {
                // Terrain writes the lit model's encoding and is lit by its pass.
                pass.set_stencil_reference(lighting_slot(item.model));
                self.terrain.draw_gbuffer(pass, targets)
            }
            DrawPass::Scene(targets) => self.terrain.draw_lit(pass, targets),
            DrawPass::Transparent => (0, 0),
        }
    }

    /// Particle integration (2 of 2): record one prepared particle batch inside the transparent queue's pass, which
    /// binds the frame (group 0) and shadow receiver (group 1) groups. Returns draw calls and triangles.
    fn draw_particles(&self, pass: &mut wgpu::RenderPass<'_>, bound: &mut BoundState, batch: usize) -> (usize, usize) {
        // The particle pass sets its own pipeline, sprite group (group 2), and instance buffer (vertex slot 0).
        *bound = BoundState {
            environment_replaced: true,
            instances: bound.instances,
            ..BoundState::default()
        };
        let instances = self.particles.draw(pass, batch);
        if instances == 0 {
            return (0, 0);
        }
        (1, (instances as usize) * 2)
    }

    // --- Passes -----------------------------------------------------------------------------------------------------

    /// Render each shadow view that needs it: each cascade in its own pass, then every rendering point-light face in
    /// one atlas pass, each through its tile's viewport. A loaded atlas keeps its cached tiles, so each face it renders
    /// is cleared to the far plane first. Returns draw calls.
    fn encode_shadow_views(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        plan: &ShadowPlan,
        draws: &FrameDraws<'_>,
    ) -> usize {
        let mut draw_calls = 0;
        for (index, view) in plan.views.iter().enumerate() {
            if let ShadowTarget::Cascade(cascade) = view.target {
                let planned = draws.plan.pass(FramePass::Cascade(cascade)).expect("rendering cascades are planned");
                let mut pass = draws.frame_targets.begin(encoder, planned);
                draw_calls += self.draw_shadow_view(&mut pass, index, view);
            }
        }
        let Some(planned) = draws.plan.pass(FramePass::PointAtlas) else {
            return draw_calls;
        };
        let keeps_tiles = planned.depth.is_some_and(|attachment| attachment.load == Load::Keep);
        let mut pass = draws.frame_targets.begin(encoder, planned);
        for (index, view) in plan.views.iter().enumerate() {
            if let ShadowTarget::PointFace(tile) = view.target {
                let (x, y, size) = (tile.x as f32, tile.y as f32, tile.size as f32);
                pass.set_viewport(x, y, size, size, 0.0, 1.0);
                pass.set_scissor_rect(tile.x, tile.y, tile.size, tile.size);
                if keeps_tiles {
                    pass.set_pipeline(self.shadows.tile_clear());
                    pass.draw(0..3, 0..1);
                    draw_calls += 1;
                }
                draw_calls += self.draw_shadow_view(&mut pass, index, view);
            }
        }
        draw_calls
    }

    /// Draw one shadow view's items into an open shadow pass; returns draw calls.
    fn draw_shadow_view(&self, pass: &mut wgpu::RenderPass<'_>, index: usize, view: &ShadowView) -> usize {
        pass.set_bind_group(0, self.shadows.caster(), &[ShadowMaps::caster_offset(index)]);
        let draw_pass = DrawPass::Shadow {
            view: index,
            matrix: view.matrix,
        };
        self.draw_items(pass, &view.items, draw_pass, None).0
    }

    /// Fill scene depth: terrain first, then opaque batches front to back, then transient opaque triangles. Clears the
    /// stencil for the G-buffer pass's lighting slots. Returns draw calls.
    fn encode_depth_prepass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        planned: &PlannedPass,
        draws: &FrameDraws<'_>,
    ) -> usize {
        let mut pass = draws.frame_targets.begin(encoder, planned);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        self.draw_items(&mut pass, &draws.opaque.depth_order, DrawPass::Depth, None).0
    }

    /// The G-buffer pass: terrain, deferred opaque batches, and transient opaque triangles write their emission and
    /// indirect light into the scene color (cleared to the background first), motion when effects need it, their
    /// encoded surfaces into the G-buffer, and their model's lighting slot into the stencil. Returns draw calls and
    /// triangles.
    fn encode_gbuffer_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        planned: &PlannedPass,
        draws: &FrameDraws<'_>,
    ) -> (usize, usize) {
        let mut pass = draws.frame_targets.begin(encoder, planned);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        pass.set_bind_group(2, draws.image_lighting, &[]);
        let deferred = draws.opaque.shading_order.iter().filter(|item| item.deferred);
        self.draw_items(&mut pass, deferred, DrawPass::GBuffer(draws.targets), Some(draws.image_lighting))
    }

    /// One full-screen lighting pass per shading model that wrote the G-buffer this frame, each adding the direct
    /// light of its pixels (selected by the stencil) to the scene color. Returns the number of passes drawn.
    fn encode_lighting_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        planned: &PlannedPass,
        draws: &FrameDraws<'_>,
    ) -> usize {
        let Some(gbuffer) = &self.gbuffer else {
            return 0;
        };
        let models = draws.opaque.deferred_models();
        if models.is_empty() {
            return 0;
        }
        let group = deferred_frame_group(
            self.gpu.device(),
            &self.pipelines.deferred_frame_layout,
            &FrameBindings {
                buffer: &self.frame_buffer,
                fog: &self.frame_fog,
                fog_sampler: &self.fog_sampler,
                lights: &self.lights,
            },
            self.post.depth_sample_view(),
            gbuffer.view(),
        );

        // Depth and stencil are only read here, while the lighting shaders sample depth through its own view.
        let mut pass = draws.frame_targets.begin(encoder, planned);
        pass.set_bind_group(0, &group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        for model in &models {
            pass.set_pipeline(self.pipelines.model(*model, ModelPass::Lighting));
            pass.set_stencil_reference(lighting_slot(*model));
            pass.draw(0..3, 0..1);
        }
        models.len()
    }

    /// A scene pass into the HDR target (and the effect attachments, when the frame writes them). Forward, it draws
    /// terrain, every opaque batch, and transient opaque triangles over the cleared background; after the deferred
    /// passes, only the batches that shade forward, over what those passes wrote. Either way it ends with the sky and,
    /// with `transparency`, the transparent queue. Returns draw calls and triangles.
    fn encode_scene_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        planned: &PlannedPass,
        draws: &FrameDraws<'_>,
        after_deferred: bool,
        transparency: bool,
    ) -> (usize, usize) {
        let mut pass = draws.frame_targets.begin(encoder, planned);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        pass.set_bind_group(2, draws.image_lighting, &[]);

        // Opaque items, each with its shading model's pipeline and its material's group (terrain binds its own).
        let forward = draws.opaque.shading_order.iter().filter(|item| !(after_deferred && item.deferred));
        let (mut draw_calls, mut triangles) =
            self.draw_items(&mut pass, forward, DrawPass::Scene(draws.targets), Some(draws.image_lighting));

        // The sky last, on the far plane, so it shades only the pixels no surface covered.
        if draws.draw_sky {
            pass.set_pipeline(self.pipelines.atmosphere(draws.targets));
            pass.set_bind_group(0, &self.atmosphere_bind_group, &[]);
            pass.draw(0..3, 0..1);
            draw_calls += 1;
        }

        if transparency {
            let (calls, drawn) = self.draw_transparent(&mut pass, draws);
            draw_calls += calls;
            triangles += drawn;
        }
        (draw_calls, triangles)
    }

    /// Transparency in its own pass over the lit scene color and opaque depth; returns draw calls and triangles.
    fn encode_transparency_pass(&self, encoder: &mut wgpu::CommandEncoder, draws: &FrameDraws<'_>) -> (usize, usize) {
        if draws.transparent.is_empty() {
            return (0, 0);
        }
        let planned = draws.plan.pass(FramePass::Transparency).expect("transparency outside the scene pass is planned");
        let mut pass = draws.frame_targets.begin(encoder, planned);
        self.draw_transparent(&mut pass, draws)
    }

    /// Draw the transparent queue back to front; returns draw calls and triangles. Binds every group it uses.
    fn draw_transparent(&self, pass: &mut wgpu::RenderPass<'_>, draws: &FrameDraws<'_>) -> (usize, usize) {
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        pass.set_bind_group(2, draws.image_lighting, &[]);
        self.draw_items(pass, draws.transparent, DrawPass::Transparent, Some(draws.image_lighting))
    }

    // --- Effects ----------------------------------------------------------------------------------------------------

    /// Detect history discontinuities and jitter the raster projection for temporal antialiasing.
    ///
    /// A presentation-clock gap longer than [`HISTORY_TIME_JUMP_SECONDS`] (a stall or a hidden window) restarts history,
    /// as does a camera cut.
    fn prepare_effects_camera(&self, effects: &mut EffectsState, camera: &CameraFrame) -> FrameCameras {
        if let Some(previous) = &effects.previous_camera {
            let distance = (camera.eye - camera.focus).length();
            let previous_forward = (previous.focus - previous.eye).normalize_or_zero();
            let forward = (camera.focus - camera.eye).normalize_or_zero();
            let time = self.presentation_time.time_seconds;
            let time_jump = time < previous.time_seconds || time - previous.time_seconds > HISTORY_TIME_JUMP_SECONDS;
            let cut = camera.eye.distance(previous.eye) > distance * 0.5
                || previous_forward.dot(forward) < 0.5_f32.cos()
                || camera.projection != previous.projection;
            if time_jump || cut {
                effects.reset_history();
            }
        }
        let jitter = if effects.runtime.jitters() {
            let (width, height) = self.render_size();
            scene_camera_jitter(effects.runtime.frame_index(), width, height)
        } else {
            [0.0; 2]
        };
        let previous = effects.previous_camera.as_ref();
        FrameCameras {
            stable: *camera,
            raster: CameraFrame {
                projection: jitter_projection(camera.projection, jitter),
                ..*camera
            },
            previous_view_projection: previous.map_or(camera.view_projection(), |previous| previous.view_projection),
            previous_view: previous.map_or(camera.view, |previous| previous.view),
        }
    }

    /// What the effects read about this frame: the scene targets after [`PostProcessor::prepare`], the cameras, and the
    /// history clock.
    fn effects_frame(&self, state: &EffectsState, cameras: &FrameCameras, size: (u32, u32)) -> EffectsFrame<'_> {
        let history = state.previous_camera.is_some();
        EffectsFrame {
            size,
            scene_view: self.post.scene_view(),
            scene_texture: self.post.scene_texture(),
            depth_view: self.post.depth_sample_view(),
            camera: cameras.stable,
            raster_projection: cameras.raster.projection,
            previous_view_projection: cameras.previous_view_projection,
            previous_view: cameras.previous_view,
            delta_seconds: if history { self.presentation_time.elapsed_seconds } else { 0.0 },
            reset_history: !history,
        }
    }

    /// Blend fog over the opaque scene, draw transparency when the plan gives it its own pass, then run the composite
    /// effects over everything and remember this frame's camera. Returns the transparency pass's draw calls and
    /// triangles.
    fn encode_effects(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        state: &mut EffectsState,
        frame: &EffectsFrame<'_>,
        draws: &FrameDraws<'_>,
        profiler: &mut FrameProfiler,
    ) -> (usize, usize) {
        let device = self.gpu.device();
        let queue = self.gpu.queue();
        if draws.plan.pass(FramePass::Fog).is_some() {
            profiler.mark(encoder, FramePhase::Fog);
            state.runtime.encode_lighting(device, queue, encoder, frame);
        }
        let (draw_calls, triangles) = if draws.plan.pass(FramePass::Transparency).is_some() {
            profiler.mark(encoder, FramePhase::Transparency);
            self.encode_transparency_pass(encoder, draws)
        } else {
            (0, 0)
        };
        profiler.mark(encoder, FramePhase::Composite);
        let motion = state.attachments.as_ref().map(SceneAttachments::motion);
        state.runtime.encode_composite(device, queue, encoder, frame, motion, self.post.exposure_texture());

        let camera = &frame.camera;
        state.previous_camera = Some(PreviousCamera {
            eye: camera.eye,
            focus: camera.focus,
            projection: camera.projection,
            view: camera.view,
            view_projection: camera.view_projection(),
            time_seconds: self.presentation_time.time_seconds,
        });
        (draw_calls, triangles)
    }

    // --- Resources --------------------------------------------------------------------------------------------------

    /// Bind `fog` (or the stand-in without volumetric fog) in the frame group, rebuilding the group only when the
    /// volume changes.
    fn bind_frame_fog(&mut self, fog: Option<&wgpu::TextureView>) {
        let view = fog.unwrap_or(&self.no_fog_view);
        if *view == self.frame_fog {
            return;
        }
        self.frame_fog = view.clone();
        self.frame_bind_group = frame_group(
            self.gpu.device(),
            &self.pipelines.frame_layout,
            &FrameBindings {
                buffer: &self.frame_buffer,
                fog: &self.frame_fog,
                fog_sampler: &self.fog_sampler,
                lights: &self.lights,
            },
        );
    }

    /// Prepare the materials transient triangles draw with.
    fn prepare_default_materials(&mut self) -> Result<(), RenderError> {
        for material in [&self.default_lit, &self.default_unlit] {
            self.materials.prepare(&self.gpu, &mut self.mipmaps, &self.models, material)?;
        }
        Ok(())
    }

    /// Upload every mesh and material in `assets` the first time each appears.
    fn upload_assets<'a>(
        &mut self,
        assets: impl Iterator<Item = (&'a MeshAsset, &'a MaterialAsset)>,
    ) -> Result<(), RenderError> {
        for (asset, material) in assets {
            self.upload_mesh(asset);
            self.materials.prepare(&self.gpu, &mut self.mipmaps, &self.models, material)?;
        }
        Ok(())
    }

    /// Compile every model pipeline the frame's passes draw with, before any pass opens. With `deferred`, items whose
    /// materials shade deferred compile their G-buffer and lighting pipelines instead of the forward one.
    fn ensure_frame_pipelines(
        &mut self,
        device: &wgpu::Device,
        targets: SceneTargets,
        opaque: &OpaqueQueues,
        transparent: &[DrawItem],
        shadow_views: &[ShadowView],
        deferred: bool,
    ) {
        let mut needed = vec![(UNLIT_MODEL_ID, ModelPass::Blend)];
        if deferred {
            needed.extend([(LIT_MODEL_ID, ModelPass::GBuffer(targets)), (LIT_MODEL_ID, ModelPass::Lighting)]);
        } else {
            needed.push((LIT_MODEL_ID, ModelPass::Scene(targets)));
        }
        for item in opaque.depth_order.iter().filter(|item| item.is_mesh()) {
            if deferred && item.deferred {
                needed.extend([(item.model, ModelPass::GBuffer(targets)), (item.model, ModelPass::Lighting)]);
            } else {
                needed.push((item.model, ModelPass::Scene(targets)));
            }
            if item.masked && self.options.depth_prepass {
                needed.push((item.model, ModelPass::MaskedDepth));
            }
        }
        needed.extend(transparent.iter().filter(|item| item.is_mesh()).map(|item| (item.model, ModelPass::Blend)));
        let masked_casters =
            shadow_views.iter().flat_map(|view| &view.items).filter(|item| item.is_mesh() && item.masked);
        needed.extend(masked_casters.map(|item| (item.model, ModelPass::MaskedCaster)));
        for (model, pass) in needed {
            self.pipelines.ensure_model(device, &self.models, model, pass);
        }
        self.pipelines.ensure_atmosphere(device, targets);
    }

    /// Set a model's pipeline unless it is already bound.
    fn bind_model(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bound: &mut BoundState,
        model: ModelId,
        model_pass: ModelPass,
    ) {
        let key = BoundPipeline::Model(model, model_pass);
        if bound.pipeline != Some(key) {
            pass.set_pipeline(self.pipelines.model(model, model_pass));
            bound.pipeline = Some(key);
        }
    }

    /// Bind a prepared material's group (group 3) unless it is already bound.
    fn bind_material(&self, pass: &mut wgpu::RenderPass<'_>, bound: &mut BoundState, material: u64) {
        if bound.material == Some(material) {
            return;
        }
        if let Some((_, group)) = self.materials.get(material) {
            pass.set_bind_group(3, group, &[]);
            bound.material = Some(material);
        }
    }

    /// Upload an asset the first time it is drawn.
    fn upload_mesh(&mut self, asset: &MeshAsset) {
        if self.meshes.contains_key(&asset.id()) {
            return;
        }
        let device = self.gpu.device();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(asset.vertices()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(asset.indices()),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.meshes.insert(
            asset.id(),
            GpuMesh {
                vertices,
                indices,
                asset: asset.downgrade(),
            },
        );
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

    /// Pack the cameras, lighting, light-cluster layout, and screen into the frame uniform block.
    fn write_frame_uniforms(&self, inputs: &FrameUniformInputs<'_>) {
        let lighting = inputs.lighting;
        let (primary_direction, primary_color) = match lighting.key {
            Some(light) => {
                (light.direction.normalize().extend(light.intensity).to_array(), light.color.with_alpha(0.0).to_array())
            }
            None => ([0.0, 1.0, 0.0, 0.0], [0.0; 4]),
        };
        let skylight = lighting.skylight;
        let cameras = inputs.cameras;
        let raster = &cameras.raster;
        let (width, height) = (inputs.render_size.0.max(1) as f32, inputs.render_size.1.max(1) as f32);
        // Screen-space noise shifts by the golden ratio each frame while temporal antialiasing averages frames.
        let noise_offset = inputs.temporal_noise.map_or(0.0, |frame| (frame as f32 * 0.618_034).fract());
        let depth = inputs.clusters.depth;
        let has_lights = if lighting.points.is_empty() { 0.0 } else { 1.0 };
        let uniforms = FrameUniforms {
            view_projection: raster.view_projection().to_cols_array_2d(),
            view: raster.view.to_cols_array_2d(),
            inverse_view_projection: raster.view_projection().inverse().to_cols_array_2d(),
            eye: raster.eye.extend(self.presentation_time.time_seconds as f32).to_array(),
            lighting: [lighting.points.len() as f32, lighting.ambient, 0.0, 0.0],
            primary_direction,
            primary_color,
            skylight: skylight.color.with_alpha(skylight.intensity).to_array(),
            ground_skylight: skylight.ground_color.with_alpha(0.0).to_array(),
            stable_view_projection: cameras.stable.view_projection().to_cols_array_2d(),
            previous_view_projection: cameras.previous_view_projection.to_cols_array_2d(),
            fog: inputs.fog_extent.map_or([0.0; 4], |extent| [1.0, extent, 0.0, 0.0]),
            debug: [self.debug_view.shader_index() as f32, noise_offset, 0.0, 0.0],
            viewport: [width, height, 1.0 / width, 1.0 / height],
            cluster_grid: [CLUSTER_COLUMNS as f32, CLUSTER_ROWS as f32, CLUSTER_SLICES as f32, has_lights],
            cluster_depth: [depth.scale, depth.bias, depth.far, 0.0],
        };
        self.gpu.queue().write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Pack the sky's inverse camera and environment block.
    fn write_atmosphere_uniforms(&self, inputs: &FrameInputs<'_>, camera: &CameraFrame) {
        let Some(environment) = inputs.environment else {
            return;
        };
        let (width, height) = self.render_size();
        let uniforms = AtmosphereUniforms {
            inverse_view_projection: camera.view_projection().inverse().to_cols_array_2d(),
            camera_position: camera.eye.extend(0.0).to_array(),
            viewport: [width as f32, height as f32, 1.0 / width as f32, 1.0 / height as f32],
            environment: environment.pack(),
        };
        self.gpu.queue().write_buffer(&self.atmosphere_buffer, 0, bytemuck::bytes_of(&uniforms));
    }
}

/// The item that draws the terrain a pass's view reaches; terrain shades with the lit model.
fn terrain_item() -> DrawItem {
    DrawItem {
        geometry: DrawGeometry::Terrain,
        model: LIT_MODEL_ID,
        material: OWN_MATERIAL,
        instances: 0..0,
        masked: false,
        deferred: true,
    }
}

/// The cascade a shadow view renders, if it is one.
fn cascade_index(target: ShadowTarget) -> Option<usize> {
    match target {
        ShadowTarget::Cascade(index) => Some(index),
        ShadowTarget::PointFace(_) => None,
    }
}

/// Which pipeline a pass has bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoundPipeline {
    /// A shading model's pipeline for one pass.
    Model(ModelId, ModelPass),
    /// The shared depth-only pre-pass.
    Depth,
    /// The shared depth-only shadow caster.
    Caster,
}

/// What an open pass has bound, so consecutive draws skip redundant state changes.
#[derive(Clone, Copy, Debug, Default)]
struct BoundState {
    pipeline: Option<BoundPipeline>,
    material: Option<u64>,
    /// Whether the instance buffer is bound at vertex slot 1 (terrain binds its own buffers there).
    instances: bool,
    /// Whether a particle batch replaced group 2 since image lighting was bound.
    environment_replaced: bool,
}

/// The scene's size for a drawing-buffer `size` and render `scale`: each axis rounded, at least one pixel unless the
/// drawing buffer has no area.
fn scaled_render_size(size: (u32, u32), scale: f32) -> (u32, u32) {
    let scale_axis = |extent: u32| {
        if extent == 0 { 0 } else { ((extent as f32 * scale).round() as u32).clamp(1, extent) }
    };
    (scale_axis(size.0), scale_axis(size.1))
}

/// A render scale within [`MIN_RENDER_SCALE`]..=[`MAX_RENDER_SCALE`]; non-finite values mean full resolution.
fn sanitized_render_scale(scale: f32) -> f32 {
    if scale.is_finite() { scale.clamp(MIN_RENDER_SCALE, MAX_RENDER_SCALE) } else { MAX_RENDER_SCALE }
}

/// Sub-pixel jitter for one frame: a Halton (2, 3) sequence of eight samples, in UV units of the render size.
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

/// Append the complete opaque transient triangles as mesh vertices; incomplete trailing vertices are dropped.
fn append_opaque_transient(triangles: &[ColorVertex], vertices: &mut Vec<MeshVertex>) {
    vertices.extend(
        triangles
            .as_chunks::<3>()
            .0
            .iter()
            .flatten()
            .map(|vertex| MeshVertex::colored(vertex.position, LinearColor::from(vertex.color))),
    );
}

/// Reject frame values the shaders cannot use safely. Lights are not checked here: invalid point and directional lights
/// are skipped instead (see [`FrameInputs`]).
fn validate_frame(inputs: &FrameInputs<'_>) -> Result<(), RenderError> {
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

/// World bounds of transient triangles, or `None` when there are none.
fn triangle_bounds(vertices: &[ColorVertex]) -> Option<Bounds> {
    let mut points = vertices.iter().map(|vertex| Vec3::from(vertex.position));
    let first = points.next()?;
    Some(points.fold(
        Bounds {
            min: first,
            max: first,
        },
        |bounds, point| Bounds {
            min: bounds.min.min(point),
            max: bounds.max.max(point),
        },
    ))
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

/// What the frame group binds (scene-common.wgsl group 0).
struct FrameBindings<'a> {
    buffer: &'a wgpu::Buffer,
    fog: &'a wgpu::TextureView,
    fog_sampler: &'a wgpu::Sampler,
    lights: &'a LightTextures,
}

impl FrameBindings<'_> {
    /// Entries for bindings 0 through 5: the uniforms, the fog volume and its sampler, and the light textures.
    fn entries(&self) -> Vec<wgpu::BindGroupEntry<'_>> {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: self.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(self.fog),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(self.fog_sampler),
            },
        ];
        entries.extend(self.lights.entries(3));
        entries
    }
}

/// The frame group every scene pass binds.
fn frame_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, bindings: &FrameBindings<'_>) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("frame"),
        layout,
        entries: &bindings.entries(),
    })
}

/// The frame group plus the scene depth and G-buffer (`sceneDepth` and `gbuffer` in deferred-lighting.wgsl), for the
/// deferred lighting passes. Built each frame they run, since the targets follow the render size.
fn deferred_frame_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    bindings: &FrameBindings<'_>,
    depth: &wgpu::TextureView,
    gbuffer: &wgpu::TextureView,
) -> wgpu::BindGroup {
    let mut entries = bindings.entries();
    entries.push(wgpu::BindGroupEntry {
        binding: 6,
        resource: wgpu::BindingResource::TextureView(depth),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: 7,
        resource: wgpu::BindingResource::TextureView(gbuffer),
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("deferred frame"),
        layout,
        entries: &entries,
    })
}

/// The stencil value marking a model's deferred surfaces; zero marks pixels no deferred surface covers.
fn lighting_slot(model: ModelId) -> u32 {
    model as u32 + 1
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
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}
