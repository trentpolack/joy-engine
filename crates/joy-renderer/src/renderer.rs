// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame orchestrator: owns pipelines, shared targets, GPU mesh uploads, and pass order.
//!
//! One frame proceeds as:
//! 1. Validate inputs, build the camera, prepare terrain, choose the point lights that matter most to the view, and cull
//!    instances against the camera.
//! 2. Lay out every draw before any pass opens: opaque batches grouped by pipeline and asset and ordered front to back,
//!    one back-to-front transparent queue (blended meshes, transient triangles, and caller runs such as particles sorted
//!    together), and each shadow view's casters culled against that cascade or point-light face. Then bin the point
//!    lights into light clusters, so every lit pass reads only the lights that reach each surface.
//! 3. Render the shadow views (each cascade, then every point-light face into its atlas tile in one pass), then (with
//!    volumetric fog) inject and integrate the fog volume, then the depth pre-pass (terrain first, as the largest
//!    occluder, then opaque batches front to back).
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

use std::collections::HashMap;
use std::ops::Range;
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
use crate::camera::{
    Camera,
    CameraFrame,
    WorldRay,
    intersect_ray_plane,
    project_to_viewport,
    screen_ray,
};
use crate::culling::{
    bounds_outside_view,
    outside_frustum,
    sphere_touches_bounds,
    world_bounds,
};
use crate::debug_view::DebugView;
use crate::deferred::GBuffer;
use crate::display_output::DisplayOutput;
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
    ScenePipelines,
    SceneTargets,
    TexturedPass,
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
    MeshInstance,
    MeshInstanceId,
    RenderScene,
    TerrainInstanceId,
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
use crate::shadows::{
    AtlasTile,
    CASCADE_BLEND_FRACTION,
    CASCADE_TEXEL_BIAS,
    CascadeRaw,
    FAR_FADE_FRACTION,
    MIN_POINT_FACE_SIZE,
    POINT_SOURCE_RADIUS,
    PointShadowRaw,
    ShadowMaps,
    ShadowOptions,
    ShadowUniforms,
    cascade_splits,
    fit_cascades,
    fit_face_sizes,
    place_faces,
    point_face_size,
    point_face_views,
    point_near_plane,
};
use crate::terrain::TerrainStats;
use crate::terrain::terrain_scene_pass::SceneTerrain;
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

/// Stencil values are eight bits and zero marks pixels no deferred surface covers, so models past this registry index
/// shade forward.
const MAX_DEFERRED_MODEL: ModelId = 254;

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
    /// Caster instances drawn across every shadow view, after culling each view's casters to that view.
    pub shadow_casters: usize,
    /// Whether directional shadow cascades were rendered this frame.
    pub shadows_active: bool,
    /// Shadow views rendered this frame: cascades plus six faces per shadowed point light.
    pub shadow_views: usize,
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
}

/// One camera-visible opaque submesh waiting to be batched.
struct OpaqueItem {
    asset: MeshAsset,
    /// Index into the asset's submeshes.
    submesh: usize,
    material: MaterialAsset,
    raw: InstanceRaw,
    /// View depth of the nearest point of the instance's bounding sphere.
    depth: f32,
}

/// One camera-visible blended submesh waiting to be sorted with the frame's other transparency.
struct BlendItem {
    /// View depth of the instance's bounds center.
    depth: f32,
    asset: MeshAsset,
    submesh: usize,
    material: MaterialAsset,
    raw: InstanceRaw,
}

/// One opaque shadow-casting submesh, visible to the camera or not.
struct CasterItem {
    asset: MeshAsset,
    submesh: usize,
    material: MaterialAsset,
    /// The material's shading model, resolved once its GPU copy is prepared ([`sort_casters`]).
    model: ModelId,
    raw: InstanceRaw,
    world: Bounds,
}

/// The frame's scene contents after camera culling, before anything is uploaded.
#[derive(Default)]
struct SceneItems {
    opaque: Vec<OpaqueItem>,
    blends: Vec<BlendItem>,
    casters: Vec<CasterItem>,
    /// This frame's model matrix of each camera-visible instance, kept as motion history when effects run.
    models: Vec<(MeshInstanceId, Mat4)>,
    visible: usize,
    culled: usize,
}

/// A recorded batch: one asset submesh drawn with one material for a contiguous instance range. Shadow-caster batches
/// share this record; unmasked casters ignore the model and material.
struct Batch {
    model: ModelId,
    material: u64,
    asset_id: u64,
    indices: Range<u32>,
    instances: Range<u32>,
    /// Whether depth-only passes must alpha-test this range.
    masked: bool,
    /// Whether the material shades in the deferred passes when they run.
    deferred: bool,
}

/// Distances along the camera's forward axis, for ordering opaque batches front to back and transparency back to front.
#[derive(Clone, Copy, Debug)]
struct ViewDepth {
    eye: Vec3,
    forward: Vec3,
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
    /// Opaque batches, front to back by their nearest instance.
    opaque: &'a [Batch],
    /// Transient opaque vertices at the start of the transient stream.
    transient_opaque: u32,
    /// The back-to-front transparent queue.
    transparent: &'a TransparentQueue,
    /// Blended submeshes, indexed by the queue's mesh order.
    blends: &'a [BlendItem],
    /// Instance offset of the first blended mesh in queue order.
    blend_start: u32,
    draw_sky: bool,
    /// Image-based lighting for PBR surfaces (the black default without it).
    image_lighting: &'a wgpu::BindGroup,
}

/// Where one shadow view renders.
#[derive(Clone, Copy, Debug)]
enum ShadowTarget {
    Cascade(usize),
    /// A point-light face's tile in the atlas.
    PointFace(AtlasTile),
}

/// One shadow view and the casters that reach it.
struct ShadowView {
    matrix: Mat4,
    target: ShadowTarget,
    batches: Vec<Batch>,
    /// Whether the transient opaque triangles reach this view.
    transient: bool,
}

/// Shadow views and receiver data prepared for one frame.
#[derive(Default)]
struct ShadowPlan {
    /// Views in pass order: cascades, then six faces per shadowed point light.
    views: Vec<ShadowView>,
    cascades: usize,
    point_lights: usize,
    /// Per selected point light, its shadow slot (`Shadows.points` in shadow-receiver.wgsl), if it casts this frame.
    light_slots: Vec<Option<usize>>,
    /// Caster instances drawn across all views.
    caster_instances: usize,
}

/// Everything that can cast into a shadow view this frame.
struct ShadowCasters<'a> {
    /// Opaque shadow-casting instances, grouped by asset.
    meshes: &'a [CasterItem],
    /// World bounds of the transient opaque triangles, if any.
    transient: Option<Bounds>,
    /// World bounds of each shadow-casting terrain instance.
    terrain: &'a [Bounds],
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

/// Renderer state lent out of `self` for one frame, so it can be borrowed alongside the renderer's own passes.
struct LentState<'a> {
    effects: Option<&'a mut EffectsState>,
    profiler: &'a mut FrameProfiler,
}

/// Which opaque surfaces a scene pass shades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScenePass {
    /// Every opaque surface, over the cleared background.
    Forward,
    /// Only the opaque batches that shade forward, over what the deferred passes wrote.
    AfterDeferred,
}

/// How one scene pass draws.
#[derive(Clone, Copy, Debug)]
struct SceneDraws {
    pass: ScenePass,
    /// Draw the transparent queue at the end of the pass.
    transparency: bool,
    /// Store depth for the effects and a later transparency pass.
    keep_depth: bool,
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

/// The engine's renderer. Borrows scenes and cameras per frame; owns all GPU resources it creates.
pub struct Renderer {
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
    // Textured transparent material and its uploaded textures (FrameInputs::transparent_draws).
    textured: TexturedPass,
    // Scene effects, when enabled and supported.
    effects: Option<EffectsState>,
    // Per-phase frame timings, while profiling is on.
    profiler: FrameProfiler,
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
    diagnostics: RendererDiagnostics,

    // The shared device; resources above keep it alive internally, so field order is not load-bearing here.
    gpu: GpuContext,
    output_format: wgpu::TextureFormat,
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
        let textured =
            TexturedPass::new(device, &pipelines.frame_layout, &pipelines.shadow_layout, capabilities.scene_format);
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
        let mipmaps = MipmapGenerator::new(device);
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

        let mut renderer = Self {
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
            textured,
            effects: None,
            profiler: FrameProfiler::default(),
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
            diagnostics,
            gpu: gpu.clone(),
            output_format,
        };
        if let Err(error) = renderer.set_scene_effects(options.scene_effects) {
            log::error!("Scene effects are disabled: {error}");
        }
        renderer
    }

    /// What this device offers scene effects.
    fn effect_support(&self) -> EffectSupport {
        let capabilities = self.gpu.capabilities();
        EffectSupport {
            compute: capabilities.compute,
            float_targets: capabilities.hdr,
        }
    }

    /// Replace the scene effects; `None` or an all-off configuration disables them. Validation failures keep the
    /// current effects. Replacing effects restarts their history and the adapted exposure.
    pub fn set_scene_effects(&mut self, options: Option<SceneEffectsOptions>) -> Result<(), SceneEffectsError> {
        if let Some(options) = &options {
            options.validate()?;
        }
        self.options.scene_effects = options;
        self.post.reset_exposure(self.gpu.queue());
        let setup = EffectsSetup {
            scene_format: self.gpu.capabilities().scene_format,
            support: self.effect_support(),
            frame_buffer: &self.frame_buffer,
            lights: &self.lights,
            shadow_layout: &self.pipelines.shadow_layout,
        };
        self.effects = match options {
            Some(options) if options.any_enabled() => Some(EffectsState {
                runtime: SceneEffectsRuntime::new(self.gpu.device(), options, &setup),
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
    /// [`RendererDiagnostics::unavailable_materials`]).
    pub fn register_shading_model(&mut self, definition: ShadingModelDefinition) -> Result<(), RenderError> {
        let common = crate::pipelines::scene_common(self.gpu.device());
        self.models.register(self.gpu.device(), definition, &common).map(|_| ())
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
        self.image_lighting.prepare(&self.gpu, &self.pipelines.image_lighting_layout, pixels, options)
    }

    /// The configured scene effects.
    pub fn scene_effects(&self) -> Option<&SceneEffectsOptions> {
        self.options.scene_effects.as_ref()
    }

    /// Which configured effects run on this device.
    pub fn scene_effects_capabilities(&self) -> SceneEffectsCapabilities {
        let options = self.options.scene_effects.unwrap_or_default();
        capabilities(&options, self.effect_support())
    }

    /// Selection and draw statistics of a scene terrain instance from the most recent frame; `None` when that frame did
    /// not prepare it (it was hidden, or outside the view without casting shadows).
    pub fn terrain_stats(&self, id: TerrainInstanceId) -> Option<&TerrainStats> {
        self.terrain.stats(id)
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

    /// Set the presentation clock for the next frame. The shell calls this before every render phase; renderers
    /// driven without a shell (tests and offline tools) advance it themselves, or temporal effects hold still.
    pub fn set_presentation_time(&mut self, time: PresentationTime) {
        self.presentation_time = time;
    }

    /// The presentation clock the next frame renders with.
    pub fn presentation_time(&self) -> PresentationTime {
        self.presentation_time
    }

    /// Drawing-buffer size in physical pixels.
    pub fn physical_size(&self) -> (u32, u32) {
        self.physical_size
    }

    /// The fraction of the drawing-buffer resolution the scene renders at ([`RendererOptions::render_scale`]).
    pub fn render_scale(&self) -> f32 {
        self.options.render_scale
    }

    /// Render the scene at `scale` times the drawing-buffer resolution from the next frame, clamped to
    /// [`MIN_RENDER_SCALE`]..=[`MAX_RENDER_SCALE`] (non-finite values mean full resolution). Lowering the scale trades
    /// sharpness for scene, effect, and bloom work, which falls with the pixel count; a change restarts temporal effect
    /// history and reallocates the scene targets on the next frame.
    pub fn set_render_scale(&mut self, scale: f32) {
        let scale = sanitized_render_scale(scale);
        if scale != self.options.render_scale {
            self.options.render_scale = scale;
            self.reset_history();
        }
    }

    /// Size in physical pixels the scene renders at: each axis of the drawing buffer times the render scale, rounded,
    /// and at least one pixel while the drawing buffer has area. Screen-space queries ([`Renderer::screen_ray`],
    /// [`Renderer::project`]) stay in logical pixels and do not depend on it.
    pub fn render_size(&self) -> (u32, u32) {
        scaled_render_size(self.physical_size, self.options.render_scale)
    }

    /// The presentation target's encoding and HDR headroom.
    pub fn display_output(&self) -> DisplayOutput {
        self.post.display_output()
    }

    /// Write for `display` from the next frame. The shell sets it when it configures the surface and whenever the
    /// display's headroom changes, so it must describe the views passed to [`Renderer::encode`]. Standard output (the
    /// default) clamps to SDR white; extended output keeps SDR white at one and tone maps highlights into the headroom.
    pub fn set_display_output(&mut self, display: DisplayOutput) {
        self.post.set_display_output(display);
        self.diagnostics.display = self.post.display_output();
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

    /// Draw (or stop drawing) the procedural sky from the next frame. Tools that light a studio preview with their own
    /// key light over a plain background turn it off; hosts that share one renderer between tools re-apply it when a
    /// tool becomes active.
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
                    options.point_atlas_size(max_texture_dimension),
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

    /// Whether opaque surfaces shade with deferred lighting where the device supports it.
    pub fn deferred(&self) -> bool {
        self.options.deferred
    }

    /// Prefer deferred lighting (`true`) or forward shading for later frames. Devices without deferred support (WebGL 2)
    /// shade forward either way; see [`deferred_supported`](Self::deferred_supported).
    pub fn set_deferred(&mut self, enabled: bool) {
        self.options.deferred = enabled;
        if !enabled {
            self.gbuffer = None;
        }
    }

    /// Whether this device can run deferred lighting.
    pub fn deferred_supported(&self) -> bool {
        self.gpu.capabilities().deferred
    }

    /// Whether the next frame shades deferred: preferred, supported, and showing the lit scene (debug views shade
    /// forward, where every surface attribute is at hand).
    fn deferred_active(&self) -> bool {
        self.options.deferred && self.deferred_supported() && self.debug_view == DebugView::Lit
    }

    /// Whether frames record a [`FrameProfile`].
    pub fn profiling(&self) -> bool {
        self.profiler.enabled()
    }

    /// Record how long each phase of every frame takes: CPU recording time always, and GPU time where the device has
    /// timestamp queries ([`GpuCapabilities::timestamps`](crate::GpuCapabilities::timestamps); not WebGL 2). Off by
    /// default; profiling adds a timestamp per phase and a small readback, and nothing while off.
    pub fn set_profiling(&mut self, enabled: bool) {
        let timestamps = self.gpu.capabilities().timestamps;
        self.profiler.set_enabled(self.gpu.device(), self.gpu.queue(), enabled, timestamps);
    }

    /// The latest complete frame profile while profiling. GPU timings are read back without stalling, so they
    /// describe a frame a few frames old ([`FrameProfile::frame`]); hosts must submit each encoded frame for them to
    /// arrive.
    pub fn frame_profile(&self) -> Option<&FrameProfile> {
        self.profiler.latest()
    }

    /// What mesh surfaces draw as: the lit scene, or one surface attribute.
    pub fn debug_view(&self) -> DebugView {
        self.debug_view
    }

    /// Draw mesh surfaces as `view` from the next frame ([`DebugView::Lit`] restores the lit scene).
    pub fn set_debug_view(&mut self, view: DebugView) {
        self.debug_view = view;
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
        let mut encoder = self.gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render frame"),
        });
        self.encode(&mut encoder, scene, camera, inputs, output)?;
        self.gpu.queue().submit([encoder.finish()]);
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
        let (width, height) = self.physical_size;
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidFrame("the drawing buffer has no area; call resize first".into()));
        }
        validate_frame(inputs)?;

        // The effects and profiler leave `self` for the frame so their state and the renderer's passes can be borrowed
        // together.
        let camera_frame = self.camera_frame(camera);
        let mut effects = self.effects.take();
        let mut profiler = std::mem::take(&mut self.profiler);
        profiler.begin_frame(self.gpu.device());
        profiler.mark(encoder, FramePhase::Prepare);
        let lent = LentState {
            effects: effects.as_mut(),
            profiler: &mut profiler,
        };
        let result = self.encode_frame(encoder, scene, inputs, output, camera_frame, lent);
        if result.is_ok() {
            profiler.end_frame(encoder);
        }
        self.profiler = profiler;
        self.effects = effects;
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
        lent: LentState<'_>,
    ) -> Result<(), RenderError> {
        let LentState {
            mut effects,
            profiler,
        } = lent;

        // The scene and its effects draw at the render size; the post chain upscales into the output size.
        let output_size = self.physical_size;
        let render_size = self.render_size();

        // Cameras. Effects may jitter the raster projection; everything that decides what to draw uses the stable one.
        let cameras = match effects.as_deref_mut() {
            Some(state) => self.prepare_effects_camera(state, &camera_frame),
            None => FrameCameras {
                stable: camera_frame,
                raster: camera_frame,
                previous_view_projection: camera_frame.view_projection(),
                previous_view: camera_frame.view,
            },
        };
        let logical_height = output_size.1 as f32 / self.scale_factor;
        let terrain =
            self.terrain.prepare(&self.gpu, &self.pipelines, encoder, scene, &camera_frame, logical_height)?;

        // Lights: the strongest point lights that can reach the view, and the key directional light when usable.
        let selected = select_point_lights(inputs.lights, &camera_frame);
        let lighting = FrameLighting {
            key: inputs.key_light().filter(DirectionalLight::is_valid),
            skylight: inputs.skylight(),
            ambient: inputs.resolved_ambient(),
            points: selected.lights,
        };

        // Scene contents, then one back-to-front queue of every transparent element. Transient opaque triangles lead
        // the transient vertex stream; the queue appends its colored triangles after them.
        let view_depth = ViewDepth::new(&camera_frame);
        let previous_models = effects.as_deref().map(|state| &state.previous_models);
        let mut items = gather_scene(scene, &camera_frame, view_depth, previous_models);

        // Every mesh and material the scene draws has a GPU copy before anything is laid out; submeshes whose material
        // names an unregistered shading model are left out.
        self.upload_scene_assets(&items)?;
        let unavailable = self.drop_unavailable(&mut items);
        let mut transient = opaque_transient_vertices(inputs.opaque_triangles);
        let transient_opaque = transient.len() as u32;
        let blend_depths: Vec<f32> = items.blends.iter().map(|item| item.depth).collect();
        let transparent = build_transparent_queue(
            &blend_depths,
            inputs.transparent_triangles,
            inputs.transparent_draws,
            view_depth.eye,
            view_depth.forward,
            &mut transient,
        );

        // Sprite textures upload before any pass opens, like meshes and materials.
        self.textured.upload_textures(&self.gpu, &mut self.mipmaps, inputs.transparent_draws)?;
        self.textured.write_vertices(&self.gpu, &transparent.textured_vertices);

        // Instances: identity (for transient triangles), opaque batches, blended meshes in queue order, then each shadow
        // view's culled casters.
        let mut raws = vec![InstanceRaw::IDENTITY];
        let opaque = batch_opaque(&mut items.opaque, &mut raws, &self.materials);
        let blend_start = raws.len() as u32;
        raws.extend(transparent.mesh_order.iter().map(|index| items.blends[*index].raw));
        sort_casters(&mut items.casters, &self.materials);
        let casters = ShadowCasters {
            meshes: &items.casters,
            transient: triangle_bounds(&inputs.opaque_triangles[..transient_opaque as usize]),
            terrain: &terrain.caster_bounds,
        };
        let shadows = self.plan_shadows(&lighting, &camera_frame, &casters, &mut raws, render_size);
        let masked_casters = shadows.views.iter().flat_map(|view| &view.batches).filter(|batch| batch.masked);
        let caster_models: Vec<ModelId> = masked_casters.map(|batch| batch.model).collect();
        self.write_instances(&raws);
        self.write_transient(&transient);

        // Light clusters for the stable camera, carrying each light's shadow slot.
        let clusters = LightClusters::build(&lighting.points, &shadows.light_slots, &camera_frame);
        self.lights.upload(self.gpu.queue(), &clusters);

        // Volumetric fog sizes its volumes for this frame before the frame group binds the integrated one.
        let device = self.gpu.device().clone();
        if let Some(state) = effects.as_deref_mut() {
            state.runtime.prepare(&device, render_size);
        }
        let fog = effects.as_deref().and_then(|state| state.runtime.fog());
        self.bind_frame_fog(fog.map(|(view, _)| view));
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
        self.write_frame_uniforms(&frame);
        self.image_lighting.write_parameters(self.gpu.queue(), inputs.image_lighting.as_ref());
        let draw_sky = self.options.atmosphere && inputs.environment.is_some();
        if draw_sky {
            self.write_atmosphere_uniforms(inputs, &cameras.raster);
        }

        // Scene targets: with effects that read motion, opaque materials write it beside their color. Transparency joins
        // the last opaque pass unless fog must apply between opaque surfaces and transparency, or the pass writes
        // attachments that blended pipelines do not.
        let targets = match effects.as_deref_mut() {
            Some(state) => state.prepare_attachments(&device, render_size),
            None => SceneTargets::Color,
        };
        // Deferred lighting runs only when some surface can use it, so scenes of unlit or forward-only materials skip
        // the G-buffer entirely.
        let deferred_surfaces =
            opaque.iter().any(|batch| batch.deferred) || transient_opaque > 0 || self.terrain.has_visible();
        let deferred = self.deferred_active() && deferred_surfaces;
        self.ensure_frame_pipelines(&device, targets, &opaque, &items.blends, &caster_models, deferred);
        self.terrain.ensure_lit(&device, targets, deferred);
        let lighting_effects = effects.as_deref().is_some_and(|state| state.runtime.has_lighting_effects());
        let transparency_in_scene = targets == SceneTargets::Color && !lighting_effects;
        self.post.prepare(&device, self.gpu.queue(), render_size, output_size);
        if deferred && self.gbuffer.as_ref().is_none_or(|gbuffer| gbuffer.size() != render_size) {
            self.gbuffer = Some(GBuffer::new(&device, render_size));
        }

        let draws = FrameDraws {
            opaque: &opaque,
            transient_opaque,
            transparent: &transparent,
            blends: &items.blends,
            blend_start,
            draw_sky,
            image_lighting: inputs
                .image_lighting
                .map_or(self.image_lighting.default_group(), |lighting| &lighting.map.bind_group),
        };
        let effects_frame = effects.as_deref().map(|state| self.effects_frame(state, &cameras, render_size));
        profiler.mark(encoder, FramePhase::Shadows);
        let mut draw_calls = self.encode_shadow_views(encoder, &shadows, transient_opaque);
        if let (Some(state), Some(frame)) = (effects.as_deref_mut(), &effects_frame)
            && state.runtime.has_lighting_effects()
        {
            profiler.mark(encoder, FramePhase::Volumetrics);
            state.runtime.encode_volume(self.gpu.queue(), encoder, frame, self.shadows.receiver());
        }
        if self.options.depth_prepass {
            profiler.mark(encoder, FramePhase::DepthPrepass);
            draw_calls += self.encode_depth_prepass(encoder, &draws);
        }

        // Opaque shading: deferred surfaces, their lighting, then whatever shades forward; or one forward pass.
        let attachments = effects.as_deref().and_then(|state| state.attachments.as_ref());
        let keep_depth = effects.is_some();
        let mut triangles = 0;
        let mut lighting_passes = 0;
        let scene_pass = if deferred {
            profiler.mark(encoder, FramePhase::GBuffer);
            let (calls, drawn) = self.encode_gbuffer_pass(encoder, inputs, &draws, attachments);
            draw_calls += calls;
            triangles += drawn;
            profiler.mark(encoder, FramePhase::Lighting);
            lighting_passes = self.encode_lighting_passes(encoder, &draws);
            draw_calls += lighting_passes;
            ScenePass::AfterDeferred
        } else {
            ScenePass::Forward
        };
        profiler.mark(encoder, FramePhase::Scene);
        let scene_draws = SceneDraws {
            pass: scene_pass,
            transparency: transparency_in_scene,
            keep_depth,
        };
        let (calls, drawn) = self.encode_scene_pass(encoder, inputs, &draws, attachments, scene_draws);
        draw_calls += calls;
        triangles += drawn;

        if let (Some(state), Some(frame)) = (effects, &effects_frame) {
            let (calls, drawn) = self.encode_effects(encoder, state, frame, &draws, !transparency_in_scene, profiler);
            draw_calls += calls;
            triangles += drawn;
            state.previous_models = items.models.iter().copied().collect();
        }

        profiler.mark(encoder, FramePhase::Post);
        self.post.encode(encoder, output);
        self.release_dropped_meshes();
        self.textured.release_dropped();
        self.materials.release_dropped();

        self.diagnostics.visible = items.visible + terrain.visible;
        self.diagnostics.culled = items.culled + terrain.culled;
        self.diagnostics.draw_calls = draw_calls;
        self.diagnostics.triangles = triangles;
        self.diagnostics.shadow_casters = shadows.caster_instances;
        self.diagnostics.shadows_active = shadows.cascades > 0;
        self.diagnostics.shadow_views = shadows.views.len();
        self.diagnostics.shadow_point_lights = shadows.point_lights;
        self.diagnostics.point_lights = lighting.points.len();
        self.diagnostics.point_lights_omitted = selected.omitted;
        self.diagnostics.cluster_light_references = clusters.reference_count();
        self.diagnostics.cluster_references_dropped = clusters.dropped;
        self.diagnostics.deferred = deferred;
        self.diagnostics.lighting_passes = lighting_passes;
        self.diagnostics.forward_opaque_batches =
            if deferred { opaque.iter().filter(|batch| !batch.deferred).count() } else { 0 };
        self.diagnostics.render_size = render_size;
        self.diagnostics.unavailable_materials = unavailable;
        Ok(())
    }

    /// Render each shadow view's culled casters, transient triangles, and terrain: each cascade in its own pass, then
    /// every point-light face in one atlas pass, each through its tile's viewport. Returns draw calls.
    fn encode_shadow_views(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        plan: &ShadowPlan,
        transient_opaque: u32,
    ) -> usize {
        let mut draw_calls = 0;
        for (index, view) in plan.views.iter().enumerate() {
            if let ShadowTarget::Cascade(cascade) = view.target {
                let mut pass = shadow_pass(encoder, "shadow cascade", self.shadows.cascade_layer(cascade));
                draw_calls += self.draw_shadow_view(&mut pass, index, view, transient_opaque);
            }
        }
        if plan.point_lights == 0 {
            return draw_calls;
        }
        let mut pass = shadow_pass(encoder, "point shadow atlas", self.shadows.atlas());
        for (index, view) in plan.views.iter().enumerate() {
            if let ShadowTarget::PointFace(tile) = view.target {
                let (x, y, size) = (tile.x as f32, tile.y as f32, tile.size as f32);
                pass.set_viewport(x, y, size, size, 0.0, 1.0);
                pass.set_scissor_rect(tile.x, tile.y, tile.size, tile.size);
                draw_calls += self.draw_shadow_view(&mut pass, index, view, transient_opaque);
            }
        }
        draw_calls
    }

    /// Draw one shadow view's casters, transient triangles, and terrain into an open shadow pass; returns draw calls.
    fn draw_shadow_view(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        index: usize,
        view: &ShadowView,
        transient_opaque: u32,
    ) -> usize {
        pass.set_bind_group(0, self.shadows.caster(), &[ShadowMaps::caster_offset(index)]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut draw_calls = self.draw_casters(pass, &view.batches);
        if view.transient && transient_opaque > 0 {
            pass.set_pipeline(&self.pipelines.caster);
            pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
            pass.draw(0..transient_opaque, 0..1);
            draw_calls += 1;
        }
        draw_calls + self.terrain.draw_shadow(pass, view.matrix)
    }

    /// Fill scene depth: terrain first, then opaque batches front to back, then transient opaque triangles. Clears the
    /// stencil for the G-buffer pass's lighting slots. Returns draw calls.
    fn encode_depth_prepass(&self, encoder: &mut wgpu::CommandEncoder, draws: &FrameDraws<'_>) -> usize {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("depth pre-pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Store,
                }),
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        let mut draw_calls = self.terrain.draw_depth(&mut pass);

        // Masked ranges alpha-test through their shading model, so cut-out holes stay open for whatever lies behind them.
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut bound = BoundState::default();
        for batch in draws.opaque {
            if batch.masked {
                self.bind_model(&mut pass, &mut bound, batch.model, ModelPass::MaskedDepth);
                self.bind_material(&mut pass, &mut bound, batch.material);
            } else if bound.pipeline != Some(BoundPipeline::Depth) {
                pass.set_pipeline(&self.pipelines.depth);
                bound.pipeline = Some(BoundPipeline::Depth);
            }
            self.draw_batch(&mut pass, batch);
            draw_calls += 1;
        }
        if draws.transient_opaque > 0 {
            pass.set_pipeline(&self.pipelines.depth);
            pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
            pass.draw(0..draws.transient_opaque, 0..1);
            draw_calls += 1;
        }
        draw_calls
    }

    /// The G-buffer pass: terrain, deferred opaque batches, and transient opaque triangles write their emission and
    /// indirect light into the scene color (cleared to the background first), motion when effects need it, their
    /// encoded surfaces into the G-buffer, and their model's lighting slot into the stencil. Returns draw calls and
    /// triangles.
    fn encode_gbuffer_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &FrameInputs<'_>,
        draws: &FrameDraws<'_>,
        attachments: Option<&SceneAttachments>,
    ) -> (usize, usize) {
        let Some(gbuffer) = &self.gbuffer else {
            return (0, 0);
        };
        let mut color_attachments = vec![scene_color_attachment(self.post.scene_view(), Some(inputs.clear_color))];
        let targets = match attachments {
            Some(attachments) => {
                color_attachments.extend(attachments.color_attachments(true));
                SceneTargets::ColorAndAttachments
            }
            None => SceneTargets::Color,
        };
        color_attachments.push(gbuffer.color_attachment());

        // After the pre-pass, depth already holds the nearest opaque surface and the stencil is clear.
        let (depth_load, stencil_load) = if self.options.depth_prepass {
            (wgpu::LoadOp::Load, wgpu::LoadOp::Load)
        } else {
            (wgpu::LoadOp::Clear(1.0), wgpu::LoadOp::Clear(0))
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("g-buffer"),
            color_attachments: &color_attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: stencil_load,
                    store: wgpu::StoreOp::Store,
                }),
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        pass.set_bind_group(2, draws.image_lighting, &[]);

        // Terrain first, as the largest occluder; it writes the lit model's encoding and is lit by its pass.
        pass.set_stencil_reference(lighting_slot(LIT_MODEL_ID));
        let (mut draw_calls, mut triangles) = self.terrain.draw_gbuffer(&mut pass, targets);

        // Deferred opaque batches, each stamping its model's lighting slot.
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut bound = BoundState::default();
        for batch in scene_order(draws.opaque, self.options.depth_prepass).into_iter().filter(|batch| batch.deferred) {
            pass.set_stencil_reference(lighting_slot(batch.model));
            self.bind_model(&mut pass, &mut bound, batch.model, ModelPass::GBuffer(targets));
            self.bind_material(&mut pass, &mut bound, batch.material);
            triangles += self.draw_batch(&mut pass, batch);
            draw_calls += 1;
        }

        // Transient opaque triangles draw with the default lit material.
        if draws.transient_opaque > 0 {
            pass.set_stencil_reference(lighting_slot(LIT_MODEL_ID));
            self.bind_model(&mut pass, &mut bound, LIT_MODEL_ID, ModelPass::GBuffer(targets));
            self.bind_material(&mut pass, &mut bound, self.default_lit.id());
            pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
            pass.draw(0..draws.transient_opaque, 0..1);
            draw_calls += 1;
            triangles += (draws.transient_opaque / 3) as usize;
        }
        (draw_calls, triangles)
    }

    /// One full-screen lighting pass per shading model that wrote the G-buffer this frame, each adding the direct
    /// light of its pixels (selected by the stencil) to the scene color. Returns the number of passes drawn.
    fn encode_lighting_passes(&self, encoder: &mut wgpu::CommandEncoder, draws: &FrameDraws<'_>) -> usize {
        let Some(gbuffer) = &self.gbuffer else {
            return 0;
        };
        let models = self.lit_deferred_models(draws);
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
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred lighting"),
            color_attachments: &[scene_color_attachment(self.post.scene_view(), None)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: None,
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        for model in &models {
            pass.set_pipeline(self.pipelines.model(*model, ModelPass::Lighting));
            pass.set_stencil_reference(lighting_slot(*model));
            pass.draw(0..3, 0..1);
        }
        models.len()
    }

    /// The models whose surfaces wrote the G-buffer this frame and respond to light, in registry order: deferred
    /// batches' models, plus the lit model for terrain and transient triangles.
    fn lit_deferred_models(&self, draws: &FrameDraws<'_>) -> Vec<ModelId> {
        let mut models: Vec<ModelId> =
            draws.opaque.iter().filter(|batch| batch.deferred).map(|batch| batch.model).collect();
        if draws.transient_opaque > 0 || self.terrain.has_visible() {
            models.push(LIT_MODEL_ID);
        }
        models.sort_unstable();
        models.dedup();
        models
    }

    /// A scene pass into the HDR target (and the effect attachments, when supplied). Forward, it draws terrain, every
    /// opaque batch, and transient opaque triangles over the cleared background; after the deferred passes, only the
    /// batches that shade forward, over what those passes wrote. Either way it ends with the sky and, when
    /// `draws.transparency` is set, transparency. Returns draw calls and triangles; `draws.keep_depth` stores depth for
    /// the effects and a later transparency pass.
    fn encode_scene_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &FrameInputs<'_>,
        draws: &FrameDraws<'_>,
        attachments: Option<&SceneAttachments>,
        scene: SceneDraws,
    ) -> (usize, usize) {
        let forward = scene.pass == ScenePass::Forward;
        let clear = forward.then_some(inputs.clear_color);
        let mut color_attachments = vec![scene_color_attachment(self.post.scene_view(), clear)];
        let targets = match attachments {
            Some(attachments) => {
                color_attachments.extend(attachments.color_attachments(forward));
                SceneTargets::ColorAndAttachments
            }
            None => SceneTargets::Color,
        };

        // After the pre-pass (or the G-buffer pass), depth already holds the nearest opaque surface and only it passes
        // the less-or-equal test.
        let depth_load =
            if self.options.depth_prepass || !forward { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(1.0) };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(if forward { "scene" } else { "scene forward" }),
            color_attachments: &color_attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: if scene.keep_depth { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        pass.set_bind_group(2, draws.image_lighting, &[]);

        // Terrain first, as the largest occluder. Terrain binds its own group 3.
        let (mut draw_calls, mut triangles) = if forward { self.terrain.draw_lit(&mut pass, targets) } else { (0, 0) };

        // Opaque batches, each with its shading model's pipeline and its material's group.
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut bound = BoundState::default();
        for batch in scene_order(draws.opaque, self.options.depth_prepass) {
            if !forward && batch.deferred {
                continue;
            }
            self.bind_model(&mut pass, &mut bound, batch.model, ModelPass::Scene(targets));
            self.bind_material(&mut pass, &mut bound, batch.material);
            triangles += self.draw_batch(&mut pass, batch);
            draw_calls += 1;
        }

        // Transient opaque triangles draw with the default lit material.
        if forward && draws.transient_opaque > 0 {
            self.bind_model(&mut pass, &mut bound, LIT_MODEL_ID, ModelPass::Scene(targets));
            self.bind_material(&mut pass, &mut bound, self.default_lit.id());
            pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
            pass.draw(0..draws.transient_opaque, 0..1);
            draw_calls += 1;
            triangles += (draws.transient_opaque / 3) as usize;
        }

        // The sky last, on the far plane, so it shades only the pixels no surface covered.
        if draws.draw_sky {
            pass.set_pipeline(self.pipelines.atmosphere(targets));
            pass.set_bind_group(0, &self.atmosphere_bind_group, &[]);
            pass.draw(0..3, 0..1);
            draw_calls += 1;
        }

        if scene.transparency {
            let (calls, drawn) = self.draw_transparent(&mut pass, draws);
            draw_calls += calls;
            triangles += drawn;
        }
        (draw_calls, triangles)
    }

    /// Transparency in its own pass over the lit scene color and stored depth; returns draw calls and triangles.
    fn encode_transparency_pass(&self, encoder: &mut wgpu::CommandEncoder, draws: &FrameDraws<'_>) -> (usize, usize) {
        if draws.transparent.steps.is_empty() {
            return (0, 0);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene transparency"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.post.scene_view(),
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            // Transparent surfaces test against the opaque depth; later passes do not read it.
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        self.draw_transparent(&mut pass, draws)
    }

    /// Draw the transparent queue back to front; returns draw calls and triangles. Binds every group it uses.
    fn draw_transparent(&self, pass: &mut wgpu::RenderPass<'_>, draws: &FrameDraws<'_>) -> (usize, usize) {
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        pass.set_bind_group(2, draws.image_lighting, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let (mut draw_calls, mut triangles) = (0, 0);
        let mut bound = BoundState::default();
        for step in &draws.transparent.steps {
            match step {
                TransparentStep::Mesh(position) => {
                    let item = &draws.blends[draws.transparent.mesh_order[*position]];
                    let (Some(mesh), Some((model, _))) =
                        (self.meshes.get(&item.asset.id()), self.materials.get(item.material.id()))
                    else {
                        continue;
                    };
                    // Textured runs rebind group 2, so model draws bind image lighting again after one.
                    if bound.environment_replaced {
                        pass.set_bind_group(2, draws.image_lighting, &[]);
                        bound.environment_replaced = false;
                    }
                    self.bind_model(pass, &mut bound, model, ModelPass::Blend);
                    self.bind_material(pass, &mut bound, item.material.id());
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    let indices = item.asset.submeshes()[item.submesh].indices.clone();
                    let instance = draws.blend_start + *position as u32;
                    triangles += indices.len() / 3;
                    pass.draw_indexed(indices, 0, instance..instance + 1);
                }
                TransparentStep::Colored(vertices) => {
                    if bound.environment_replaced {
                        pass.set_bind_group(2, draws.image_lighting, &[]);
                        bound.environment_replaced = false;
                    }
                    self.bind_model(pass, &mut bound, UNLIT_MODEL_ID, ModelPass::Blend);
                    self.bind_material(pass, &mut bound, self.default_unlit.id());
                    pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                    pass.draw(vertices.clone(), 0..1);
                    triangles += vertices.len() / 3;
                }
                TransparentStep::Textured {
                    texture,
                    vertices,
                } => {
                    // The textured pass sets its own pipeline and groups.
                    bound = BoundState {
                        environment_replaced: true,
                        ..BoundState::default()
                    };
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

    /// Blend fog over the opaque scene, draw transparency when `transparency_pending`, then run the composite effects
    /// over everything and remember this frame's camera. Returns the transparency pass's draw calls and triangles.
    fn encode_effects(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        state: &mut EffectsState,
        frame: &EffectsFrame<'_>,
        draws: &FrameDraws<'_>,
        transparency_pending: bool,
        profiler: &mut FrameProfiler,
    ) -> (usize, usize) {
        let device = self.gpu.device();
        let queue = self.gpu.queue();
        if state.runtime.has_lighting_effects() {
            profiler.mark(encoder, FramePhase::Fog);
            state.runtime.encode_lighting(device, queue, encoder, frame);
        }
        let (draw_calls, triangles) = if transparency_pending {
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

    /// Upload every mesh and material the frame's items draw (and the materials transient triangles use), the first
    /// time each appears.
    fn upload_scene_assets(&mut self, items: &SceneItems) -> Result<(), RenderError> {
        let opaque = items.opaque.iter().map(|item| (&item.asset, &item.material));
        let blends = items.blends.iter().map(|item| (&item.asset, &item.material));
        let casters = items.casters.iter().map(|item| (&item.asset, &item.material));
        for (asset, material) in opaque.chain(blends).chain(casters) {
            self.upload_mesh(asset);
            self.materials.prepare(&self.gpu, &mut self.mipmaps, &self.models, material)?;
        }
        for material in [&self.default_lit, &self.default_unlit] {
            self.materials.prepare(&self.gpu, &mut self.mipmaps, &self.models, material)?;
        }
        Ok(())
    }

    /// Remove items whose material names a shading model nothing registered; returns how many camera-visible
    /// submeshes were removed.
    fn drop_unavailable(&self, items: &mut SceneItems) -> usize {
        let available = |material: &MaterialAsset| self.materials.get(material.id()).is_some();
        let before = items.opaque.len() + items.blends.len();
        items.opaque.retain(|item| available(&item.material));
        items.blends.retain(|item| available(&item.material));
        items.casters.retain(|item| available(&item.material));
        before - (items.opaque.len() + items.blends.len())
    }

    /// Compile every model pipeline the frame's passes draw with, before any pass opens. With `deferred`, batches whose
    /// materials shade deferred compile their G-buffer and lighting pipelines instead of the forward one.
    fn ensure_frame_pipelines(
        &mut self,
        device: &wgpu::Device,
        targets: SceneTargets,
        opaque: &[Batch],
        blends: &[BlendItem],
        caster_models: &[ModelId],
        deferred: bool,
    ) {
        let mut needed = vec![(UNLIT_MODEL_ID, ModelPass::Blend)];
        if deferred {
            needed.extend([(LIT_MODEL_ID, ModelPass::GBuffer(targets)), (LIT_MODEL_ID, ModelPass::Lighting)]);
        } else {
            needed.push((LIT_MODEL_ID, ModelPass::Scene(targets)));
        }
        for batch in opaque {
            if deferred && batch.deferred {
                needed.extend([(batch.model, ModelPass::GBuffer(targets)), (batch.model, ModelPass::Lighting)]);
            } else {
                needed.push((batch.model, ModelPass::Scene(targets)));
            }
            if batch.masked && self.options.depth_prepass {
                needed.push((batch.model, ModelPass::MaskedDepth));
            }
        }
        for item in blends {
            if let Some((model, _)) = self.materials.get(item.material.id()) {
                needed.push((model, ModelPass::Blend));
            }
        }
        needed.extend(caster_models.iter().map(|model| (*model, ModelPass::MaskedCaster)));
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

    /// Draw caster batches with the depth-only caster, or their model's alpha-tested caster when masked; returns the
    /// number of draw calls.
    fn draw_casters(&self, pass: &mut wgpu::RenderPass<'_>, batches: &[Batch]) -> usize {
        let mut bound = BoundState::default();
        for batch in batches {
            if batch.masked {
                self.bind_model(pass, &mut bound, batch.model, ModelPass::MaskedCaster);
                self.bind_material(pass, &mut bound, batch.material);
            } else if bound.pipeline != Some(BoundPipeline::Caster) {
                pass.set_pipeline(&self.pipelines.caster);
                bound.pipeline = Some(BoundPipeline::Caster);
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

    /// Choose this frame's shadow views, cull each view's casters, and upload the view matrices and receiver uniforms.
    ///
    /// The key light casts cascades when it is lit and set to cast. Selected point lights set to cast take shadow slots
    /// in importance order, up to the configured budget, when their range touches a caster (a light whose reach holds
    /// no caster would see empty faces). Their faces are sized by how much of the `render_size` view each light
    /// reaches and placed in the atlas, shrinking the least important lights' faces when the atlas is full. Every view
    /// draws only the casters whose bounds reach its volume, and caster instances are appended to `raws`.
    fn plan_shadows(
        &mut self,
        lighting: &FrameLighting,
        camera: &CameraFrame,
        casters: &ShadowCasters<'_>,
        raws: &mut Vec<InstanceRaw>,
        render_size: (u32, u32),
    ) -> ShadowPlan {
        let mut plan = ShadowPlan {
            light_slots: vec![None; lighting.points.len()],
            ..ShadowPlan::default()
        };
        let mut uniforms = ShadowUniforms::disabled();
        let Some(options) = self.options.shadows.filter(|_| !casters.is_empty()) else {
            self.shadows.write_uniforms(self.gpu.queue(), &uniforms);
            return plan;
        };
        let quality = options.quality;
        let max_texture_dimension = self.gpu.capabilities().max_texture_dimension;
        uniforms.counts = [0.0, quality.blocker_sample_count() as f32, quality.filter_sample_count() as f32, 0.0];
        uniforms.parameters = [FAR_FADE_FRACTION, 0.0, 0.0, 0.0];

        // Directional cascades. Each cascade's volume reaches toward the light only as far as the casters above it, so
        // casters outside every cascade (including those beyond the shadow distance) are never drawn.
        if let Some(light) = lighting.key {
            let distance = options.shadow_distance.min(camera.far).max(camera.near);
            if light.casts_shadow && light.is_lit() && distance > camera.near {
                let count = quality.cascade_count();
                let splits = cascade_splits(camera.near, distance, count);
                let map_size = options.directional_map_size(max_texture_dimension);
                let fits = fit_cascades(camera, light.direction, &splits, count, map_size, distance, &casters.bounds());
                for (index, fit) in fits.iter().enumerate() {
                    uniforms.cascades[index] = CascadeRaw {
                        matrix: fit.matrix.to_cols_array_2d(),
                        extent: [
                            splits[index],
                            fit.texel,
                            fit.depth_range,
                            options.normal_bias.max(fit.texel * CASCADE_TEXEL_BIAS),
                        ],
                    };
                    plan.add_view(fit.matrix, ShadowTarget::Cascade(index), casters, raws);
                }
                uniforms.counts[0] = fits.len() as f32;
                uniforms.directional = [distance, 1.0, light.angular_radius.tan(), CASCADE_BLEND_FRACTION];
                plan.cascades = fits.len();
            }
        }

        // Point lights: the most important casting lights that reach a caster, their face sizes fitted to the atlas,
        // which grows (up to the quality's size) to what the frame asks for.
        let largest_face = options.point_map_size(max_texture_dimension);
        let mut shadowed: Vec<usize> = Vec::new();
        let mut face_sizes: Vec<u32> = Vec::new();
        for (index, light) in lighting.points.iter().enumerate() {
            if shadowed.len() == options.point_light_budget() {
                break;
            }
            if light.casts_shadow && casters.touch_sphere(light.position, light.range) {
                shadowed.push(index);
                face_sizes.push(point_face_size(light.position, light.range, camera, render_size.1, largest_face));
            }
        }
        if !shadowed.is_empty() {
            self.shadows.reserve_atlas(self.gpu.device(), &face_sizes);
        }
        let atlas_size = self.shadows.allocated_atlas_size();
        fit_face_sizes(&mut face_sizes, atlas_size, MIN_POINT_FACE_SIZE);
        shadowed.truncate(face_sizes.len());
        let tiles = place_faces(&face_sizes, atlas_size);
        let texel = 1.0 / atlas_size as f32;
        for (slot, (light_index, faces)) in shadowed.iter().zip(&tiles).enumerate() {
            let light = &lighting.points[*light_index];
            let origin = |face: usize| [faces[face].x as f32 * texel, faces[face].y as f32 * texel];
            let pair = |first: usize| {
                let [x0, y0] = origin(first);
                let [x1, y1] = origin(first + 1);
                [x0, y0, x1, y1]
            };
            let face_size = faces[0].size as f32;
            uniforms.points[slot] = PointShadowRaw {
                position_range: light.position.extend(light.range).to_array(),
                parameters: [point_near_plane(light.range), POINT_SOURCE_RADIUS, options.normal_bias, 1.0],
                tiles: [pair(0), pair(2), pair(4)],
                tile: [face_size * texel, face_size, 0.0, 0.0],
            };
            plan.light_slots[*light_index] = Some(slot);
            for (face, matrix) in point_face_views(light.position, light.range).into_iter().enumerate() {
                plan.add_view(matrix, ShadowTarget::PointFace(faces[face]), casters, raws);
            }
        }
        plan.point_lights = shadowed.len();
        uniforms.counts[3] = plan.point_lights as f32;

        let device = self.gpu.device().clone();
        let matrices: Vec<Mat4> = plan.views.iter().map(|view| view.matrix).collect();
        self.shadows.write_casters(&device, self.gpu.queue(), &matrices);
        self.shadows.write_uniforms(self.gpu.queue(), &uniforms);
        plan
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

impl ViewDepth {
    fn new(camera: &CameraFrame) -> Self {
        // The view matrix's third row is the camera's backward axis in world space.
        let backward = camera.view.row(2).truncate();
        Self {
            eye: camera.eye,
            forward: (-backward).try_normalize().unwrap_or(Vec3::NEG_Z),
        }
    }

    /// Distance of `point` in front of the camera.
    fn of(self, point: Vec3) -> f32 {
        (point - self.eye).dot(self.forward)
    }

    /// View depth of the nearest point of the sphere around `bounds`.
    fn nearest(self, bounds: Bounds) -> f32 {
        self.of(bounds.center()) - ((bounds.max - bounds.min).length() * 0.5)
    }
}

impl ShadowPlan {
    /// Add one view, culling the casters to it.
    fn add_view(
        &mut self,
        matrix: Mat4,
        target: ShadowTarget,
        casters: &ShadowCasters<'_>,
        raws: &mut Vec<InstanceRaw>,
    ) {
        let start = raws.len();
        let batches = casters.batches_for(matrix, raws);
        self.caster_instances += raws.len() - start;
        let transient = casters.transient.is_some_and(|bounds| !bounds_outside_view(bounds, matrix));
        self.views.push(ShadowView {
            matrix,
            target,
            batches,
            transient,
        });
    }
}

impl ShadowCasters<'_> {
    fn is_empty(&self) -> bool {
        self.meshes.is_empty() && self.transient.is_none() && self.terrain.is_empty()
    }

    /// World bounds of every caster.
    fn bounds(&self) -> Vec<Bounds> {
        let meshes = self.meshes.iter().map(|item| item.world);
        meshes.chain(self.transient).chain(self.terrain.iter().copied()).collect()
    }

    /// Whether any caster's bounds touch the sphere at `center` with `radius`.
    fn touch_sphere(&self, center: Vec3, radius: f32) -> bool {
        let touches = |bounds: &Bounds| sphere_touches_bounds(center, radius, *bounds);
        self.meshes.iter().any(|item| touches(&item.world))
            || self.transient.as_ref().is_some_and(touches)
            || self.terrain.iter().any(touches)
    }

    /// Batches of the mesh casters whose bounds reach `view_projection`, appending their instances to `raws`. Casters
    /// arrive sorted by [`sort_casters`], so each group shares one draw.
    fn batches_for(&self, view_projection: Mat4, raws: &mut Vec<InstanceRaw>) -> Vec<Batch> {
        let mut batches = Vec::new();
        for group in self.meshes.chunk_by(|left, right| caster_key(left) == caster_key(right)) {
            let start = raws.len() as u32;
            let reaching = group.iter().filter(|item| !bounds_outside_view(item.world, view_projection));
            raws.extend(reaching.map(|item| item.raw));
            let end = raws.len() as u32;
            if end > start {
                let (masked, model, material, asset_id, submesh) = caster_key(&group[0]);
                batches.push(Batch {
                    model,
                    material,
                    asset_id,
                    indices: group[0].asset.submeshes()[submesh].indices.clone(),
                    instances: start..end,
                    masked,
                    deferred: false,
                });
            }
        }
        batches
    }
}

/// Cull the scene's instances against the camera and sort their submeshes into opaque, blended, and caster items.
///
/// Shadow casters include camera-culled geometry, so off-screen objects still cast into the view. With effect history,
/// each instance carries its previous model matrix for motion vectors.
fn gather_scene(
    scene: &RenderScene,
    camera: &CameraFrame,
    view_depth: ViewDepth,
    previous_models: Option<&HashMap<MeshInstanceId, Mat4>>,
) -> SceneItems {
    let view_projection = camera.view_projection();
    let mut items = SceneItems::default();
    for (id, instance) in scene.iter() {
        let (model, shown) = scene.compose(instance.parent, instance.transform.matrix(), instance.visible);
        if !shown || instance.opacity <= 0.0 {
            continue;
        }
        let previous = previous_models.and_then(|models| models.get(&id)).copied().unwrap_or(model);
        let raw = instance_raw(instance, model, previous);
        let world = world_bounds(instance.asset.bounds(), model);
        let transparent = instance.is_transparent();

        // Opaque, unblended submeshes cast shadows; blended ones and transparent instances do not.
        if instance.casts_shadow && !transparent {
            for (index, submesh) in instance.asset.submeshes().iter().enumerate() {
                let material = instance.material(submesh.slot);
                if !material.blends() {
                    let (asset, material) = (instance.asset.clone(), material.clone());
                    items.casters.push(CasterItem {
                        asset,
                        submesh: index,
                        material,
                        model: 0,
                        raw,
                        world,
                    });
                }
            }
        }

        if outside_frustum(instance.asset.bounds(), model, view_projection) {
            items.culled += 1;
            continue;
        }
        items.visible += 1;
        if previous_models.is_some() {
            items.models.push((id, model));
        }
        queue_instance(instance, raw, world, view_depth, transparent, &mut items);
    }
    items
}

/// Route one camera-visible instance's submeshes into the opaque items and the blended items: every submesh of a
/// transparent instance blends, and otherwise each submesh follows its material.
fn queue_instance(
    instance: &MeshInstance,
    raw: InstanceRaw,
    world: Bounds,
    view_depth: ViewDepth,
    transparent: bool,
    items: &mut SceneItems,
) {
    let center_depth = view_depth.of(world.center());
    let nearest_depth = view_depth.nearest(world);
    for (index, submesh) in instance.asset.submeshes().iter().enumerate() {
        let material = instance.material(submesh.slot).clone();
        let asset = instance.asset.clone();
        if transparent || material.blends() {
            items.blends.push(BlendItem {
                depth: center_depth,
                asset,
                submesh: index,
                material,
                raw,
            });
        } else {
            items.opaque.push(OpaqueItem {
                asset,
                submesh: index,
                material,
                raw,
                depth: nearest_depth,
            });
        }
    }
}

/// Group opaque items into instanced batches and append their instances to `raws`.
///
/// Items of one model, material, asset, and submesh share a batch, nearest instance first, and batches are ordered by
/// their nearest instance, so depth fills front to back and the surfaces behind fail the depth test before shading.
/// Every item's material is prepared (see [`Renderer::drop_unavailable`]).
fn batch_opaque(items: &mut [OpaqueItem], raws: &mut Vec<InstanceRaw>, materials: &MaterialCache) -> Vec<Batch> {
    let key = |item: &OpaqueItem| {
        let model = materials.get(item.material.id()).map_or(usize::MAX, |(model, _)| model);
        (model, item.material.id(), item.asset.id(), item.submesh)
    };
    items.sort_by(|left, right| key(left).cmp(&key(right)).then(left.depth.total_cmp(&right.depth)));
    let mut groups: Vec<&[OpaqueItem]> = items.chunk_by(|left, right| key(left) == key(right)).collect();
    groups.sort_by(|left, right| left[0].depth.total_cmp(&right[0].depth));

    let mut batches = Vec::new();
    for group in groups {
        let start = raws.len() as u32;
        raws.extend(group.iter().map(|item| item.raw));
        let (model, material, asset_id, submesh) = key(&group[0]);
        batches.push(Batch {
            model,
            material,
            asset_id,
            indices: group[0].asset.submeshes()[submesh].indices.clone(),
            instances: start..raws.len() as u32,
            masked: group[0].material.masks(),
            deferred: materials.deferred(material) && model <= MAX_DEFERRED_MODEL,
        });
    }
    batches
}

/// Resolve each caster's shading model, then order casters so each shared draw is contiguous: unmasked casters group
/// by asset and submesh whatever their material, and masked ones also by model and material.
fn sort_casters(casters: &mut [CasterItem], materials: &MaterialCache) {
    for item in casters.iter_mut() {
        item.model = materials.get(item.material.id()).map_or(0, |(model, _)| model);
    }
    casters.sort_by_key(caster_key);
}

/// The draw a caster shares: `(masked, model, material, asset, submesh)`, with the model and material zeroed for
/// unmasked casters, which all draw with the depth-only caster.
fn caster_key(item: &CasterItem) -> (bool, ModelId, u64, u64, usize) {
    if !item.material.masks() {
        return (false, 0, 0, item.asset.id(), item.submesh);
    }
    (true, item.model, item.material.id(), item.asset.id(), item.submesh)
}

/// The scene pass's batch order. After the depth pre-pass only the nearest surface passes the depth test, so batches
/// group by model and material (nearest first within each) to save state changes; without it they keep their
/// front-to-back order.
fn scene_order(batches: &[Batch], depth_prepass: bool) -> Vec<&Batch> {
    let mut order: Vec<&Batch> = batches.iter().collect();
    if depth_prepass {
        order.sort_by_key(|batch| (batch.model, batch.material));
    }
    order
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
    /// Whether a textured run replaced group 2 since image lighting was bound.
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

/// Pack an instance's transform and its previous frame's, tint, opacity, and shadow flag.
fn instance_raw(instance: &MeshInstance, model: Mat4, previous_model: Mat4) -> InstanceRaw {
    InstanceRaw {
        model: model.to_cols_array_2d(),
        tint: instance.tint.with_alpha(instance.opacity.clamp(0.0, 1.0)).to_array(),
        flags: [if instance.receives_shadow { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        previous_model: previous_model.to_cols_array_2d(),
    }
}

/// Convert the complete opaque transient triangles to mesh vertices; incomplete trailing vertices are dropped.
fn opaque_transient_vertices(triangles: &[ColorVertex]) -> Vec<MeshVertex> {
    triangles
        .as_chunks::<3>()
        .0
        .iter()
        .flatten()
        .map(|vertex| MeshVertex::colored(vertex.position, LinearColor::from(vertex.color)))
        .collect()
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

/// The scene color attachment of an opaque pass: cleared to `clear` by the frame's first pass, loaded by later ones.
fn scene_color_attachment(
    view: &wgpu::TextureView,
    clear: Option<LinearColor>,
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    let load = match clear {
        Some(color) => wgpu::LoadOp::Clear(wgpu::Color {
            r: f64::from(color.red),
            g: f64::from(color.green),
            b: f64::from(color.blue),
            a: f64::from(color.alpha),
        }),
        None => wgpu::LoadOp::Load,
    };
    Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
        depth_slice: None,
    })
}

/// A depth-only shadow pass into `target`, cleared to the far plane.
fn shadow_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: target,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
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
