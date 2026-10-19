// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame orchestrator: owns pipelines, shared targets, GPU mesh uploads, and pass order.
//!
//! One frame proceeds as:
//! 1. Validate inputs, build the camera, prepare terrain, choose the point lights that matter most to the view, and cull
//!    instances against the camera.
//! 2. Lay out every draw before any pass opens: opaque batches grouped by pipeline and asset and ordered front to back,
//!    one back-to-front transparent queue (blended meshes, transient triangles, and caller runs such as particles sorted
//!    together), and each shadow view's casters culled against that cascade or point-light face.
//! 3. Render the shadow views, then the depth pre-pass (terrain first, as the largest occluder, then opaque batches front
//!    to back), then the scene pass into the HDR target. Opaque surfaces test depth less-or-equal against the pre-pass,
//!    so each visible pixel is shaded once, and the sky fills the uncovered pixels on the far plane last. With scene
//!    effects, opaque materials also write normals, roughness, and motion into attachments in that same pass.
//! 4. Draw transparency after the lighting effects that read opaque surfaces (ambient occlusion, reflections, global
//!    illumination, fog, and outlines) and before the composite effects (temporal antialiasing, motion blur, and
//!    exposure) and post-processing, so every post effect sees transparent surfaces. Without lighting effects,
//!    transparency draws at the end of the scene pass.
//! 5. Post-process into the caller's presentation view.
//!
//! Steps 3 and 4 draw at the render size: the drawing-buffer size times the render scale. The post chain upscales that
//! scene into the output, so a scale below one trades sharpness for scene, effect, and bloom work while antialiasing,
//! grain, and any UI drawn afterwards keep the full output resolution.
//!
//! Materials define shading; the renderer defines frame-wide invariants and submission order. Temporal antialiasing
//! jitters the raster projection by a Halton sequence; history resets on camera cuts, presentation-clock jumps, and
//! projection changes.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Weak;

use joy_core::math::{
    Mat4,
    Vec2,
    Vec3,
};
use joy_core::{
    DirectionalLight,
    LinearColor,
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
use crate::display_output::DisplayOutput;
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
    PresentationTime,
};
use crate::gpu::GpuContext;
use crate::materials::{
    AtmosphereUniforms,
    CasterBias,
    FrameUniforms,
    GpuPointLight,
    InstanceRaw,
    MaterialMaps,
    ScenePipelines,
    SceneTargets,
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
use crate::point_lights::select_point_lights;
use crate::post_process::PostProcessSettings;
use crate::postprocessor::PostProcessor;
use crate::scene::{
    MeshInstance,
    MeshInstanceId,
    RenderScene,
    Shading,
    TerrainInstanceId,
};
use crate::scene_effects::{
    EffectStage,
    EffectsFrame,
    SceneAttachments,
    SceneEffectsCapabilities,
    SceneEffectsError,
    SceneEffectsOptions,
    SceneEffectsRuntime,
    SceneVolumeStats,
    VolumeCamera,
    VolumeLights,
    capabilities,
};
use crate::shadows::{
    CASCADE_BLEND_FRACTION,
    CASCADE_TEXEL_BIAS,
    FAR_FADE_FRACTION,
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
    /// bloom, and blurs run at the reduced size and the post chain upscales the result, while antialiasing, grain, and
    /// any UI drawn afterwards keep the full output resolution.
    pub render_scale: f32,
    /// Scene effects (ambient occlusion, reflections, antialiasing, ...); replace later with
    /// [`Renderer::set_scene_effects`]. Ignored on backends without scene effects (WebGL 2).
    pub scene_effects: Option<SceneEffectsOptions>,
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
    /// Draw calls recorded for the shadow, depth pre-pass, scene, and transparency passes.
    pub draw_calls: usize,
    /// Triangles submitted to the scene and transparency passes.
    pub triangles: usize,
    /// Caster instances drawn across every shadow view, after culling each view's casters to that view.
    pub shadow_casters: usize,
    /// Whether directional shadow cascades were rendered this frame.
    pub shadows_active: bool,
    /// Shadow views rendered this frame: cascades plus six faces per shadowed point light.
    pub shadow_views: usize,
    /// Point lights that cast shadows this frame.
    pub shadow_point_lights: usize,
    /// Point lights drawn this frame (at most [`MAX_POINT_LIGHTS`]).
    pub point_lights: usize,
    /// Supplied point lights left out this frame: invalid, unable to reach the view, or beyond the cap.
    pub point_lights_omitted: usize,
}

/// Which surface pipeline draws an opaque batch; scene-pass order after the depth pre-pass.
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

/// One camera-visible opaque instance waiting to be batched.
struct OpaqueItem {
    pipeline: OpaquePipeline,
    asset: MeshAsset,
    raw: InstanceRaw,
    /// View depth of the nearest point of the instance's bounding sphere.
    depth: f32,
}

/// One camera-visible blended submesh waiting to be sorted with the frame's other transparency.
struct BlendItem {
    /// View depth of the instance's bounds center.
    depth: f32,
    pipeline: BlendPipeline,
    asset: MeshAsset,
    indices: Range<u32>,
    material: Option<u64>,
    raw: InstanceRaw,
}

/// One opaque shadow-casting instance, visible to the camera or not.
struct CasterItem {
    asset: MeshAsset,
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

/// A recorded batch: one asset submesh drawn for a contiguous instance range.
/// Shadow-caster batches share this record; the caster passes ignore `pipeline`.
struct Batch {
    pipeline: OpaquePipeline,
    asset_id: u64,
    indices: Range<u32>,
    instances: Range<u32>,
    /// Material maps for the PBR pass and masked depth and caster passes; `None` binds the white default.
    material: Option<u64>,
    /// Whether depth-only passes must alpha-test this range.
    masked: bool,
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
    /// This frame's raster jitter in UV units.
    jitter: [f32; 2],
    /// The previous frame's stable view-projection; this frame's without history.
    previous_view_projection: Mat4,
    /// Change in jitter since the previous frame, in UV units.
    jitter_delta: [f32; 2],
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
    PointFace { slot: usize, face: usize },
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
    /// Per selected point light, its shadow slot plus one, or zero (`PointLight.color.a` in scene-common.wgsl).
    slots: [f32; MAX_POINT_LIGHTS],
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

/// The last submitted camera, for history resets and reprojection.
struct PreviousCamera {
    eye: Vec3,
    focus: Vec3,
    projection: Mat4,
    view_projection: Mat4,
    raster_view_projection: Mat4,
    jitter: [f32; 2],
    /// Presentation seconds when it was drawn.
    time_seconds: f64,
}

/// Scene effects and the frame-to-frame state they need.
struct EffectsState {
    runtime: SceneEffectsRuntime,
    /// The normal, roughness, and motion targets the scene pass writes, when the effects read them.
    attachments: Option<SceneAttachments>,
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

    /// Allocate the scene-pass attachments the effects read for a `size` frame (or release them), and return the
    /// targets the scene pass writes.
    fn prepare_attachments(&mut self, device: &wgpu::Device, size: (u32, u32)) -> SceneTargets {
        let requirements = self.runtime.options().requirements();
        if !(requirements.normal_texture || requirements.velocity_texture || requirements.taa_velocity_texture) {
            self.attachments = None;
            return SceneTargets::Color;
        }
        if self.attachments.as_ref().is_none_or(|attachments| attachments.size() != size) {
            self.attachments = Some(SceneAttachments::new(device, size));
        }
        SceneTargets::ColorAndAttachments
    }
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
    atmosphere_buffer: wgpu::Buffer,
    atmosphere_bind_group: wgpu::BindGroup,
    shadows: ShadowMaps,
    // PBR material maps (with generated mips) and image-based lighting (the default group is black with zero
    // intensity).
    mipmaps: MipmapGenerator,
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

    // Configuration, drawing size, and the shell's presentation clock.
    options: RendererOptions,
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

        let mipmaps = MipmapGenerator::new(device);
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
            display: DisplayOutput::STANDARD,
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
            mipmaps,
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
            options: RendererOptions { render_scale: sanitized_render_scale(options.render_scale), ..options.clone() },
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
                attachments: None,
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
        self.post.set_display_output(self.gpu.device(), display);
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

        // The effects leave `self` for the frame so their state and the renderer's passes can be borrowed together.
        let camera_frame = self.camera_frame(camera);
        let mut effects = self.effects.take();
        let result = self.encode_frame(encoder, scene, inputs, output, camera_frame, effects.as_mut());
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
        mut effects: Option<&mut EffectsState>,
    ) -> Result<(), RenderError> {
        // The scene and its effects draw at the render size; the post chain upscales into the output size.
        let output_size = self.physical_size;
        let render_size = self.render_size();

        // Cameras. Effects may jitter the raster projection; everything that decides what to draw uses the stable one.
        let cameras = match effects.as_deref_mut() {
            Some(state) => self.prepare_effects_camera(state, &camera_frame),
            None => FrameCameras {
                stable: camera_frame,
                raster: camera_frame,
                jitter: [0.0; 2],
                previous_view_projection: camera_frame.view_projection(),
                jitter_delta: [0.0; 2],
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

        // Every texture, mesh, and material the frame draws has a GPU copy before any pass opens.
        self.textured.upload_textures(&self.gpu, &mut self.mipmaps, inputs.transparent_draws)?;
        self.textured.write_vertices(&self.gpu, &transparent.textured_vertices);
        self.upload_scene_assets(&items)?;

        // Instances: identity (for transient triangles), opaque batches, blended meshes in queue order, then each shadow
        // view's culled casters.
        let mut raws = vec![InstanceRaw::IDENTITY];
        let opaque = batch_opaque(&mut items.opaque, &mut raws);
        let blend_start = raws.len() as u32;
        raws.extend(transparent.mesh_order.iter().map(|index| items.blends[*index].raw));
        items.casters.sort_by_key(|item| item.asset.id());
        let casters = ShadowCasters {
            meshes: &items.casters,
            transient: triangle_bounds(&inputs.opaque_triangles[..transient_opaque as usize]),
            terrain: &terrain.caster_bounds,
        };
        let shadows = self.plan_shadows(&lighting, &camera_frame, &casters, &mut raws);
        self.write_instances(&raws);
        self.write_transient(&transient);

        // Frame, sky, and image-lighting uniforms.
        self.write_frame_uniforms(&lighting, &shadows.slots, &cameras);
        let image_parameters = inputs
            .image_lighting
            .map_or([0.0; 4], |lighting| [lighting.intensity, lighting.rotation, lighting.map.max_mip, 0.0]);
        self.gpu.queue().write_buffer(&self.image_lighting_buffer, 0, bytemuck::cast_slice(&image_parameters));
        let draw_sky = self.options.atmosphere && inputs.environment.is_some();
        if draw_sky {
            self.write_atmosphere_uniforms(inputs, &cameras.raster);
        }

        // Scene targets: with effects that read surfaces, opaque materials write the attachments beside their color.
        // Transparency joins the scene pass unless lighting effects must run between opaque surfaces and transparency.
        let device = self.gpu.device().clone();
        let targets = match effects.as_deref_mut() {
            Some(state) => state.prepare_attachments(&device, render_size),
            None => SceneTargets::Color,
        };
        self.pipelines.ensure_opaque(&device, targets);
        self.terrain.ensure_lit(&device, targets);
        let lighting_effects = effects.as_deref().is_some_and(|state| state.runtime.has_lighting_effects());
        let transparency_in_scene = targets == SceneTargets::Color && !lighting_effects;
        let time_seconds = self.presentation_time.time_seconds as f32;
        self.post.prepare(&device, self.gpu.queue(), render_size, output_size, time_seconds);

        let draws = FrameDraws {
            opaque: &opaque,
            transient_opaque,
            transparent: &transparent,
            blends: &items.blends,
            blend_start,
            draw_sky,
            image_lighting: inputs
                .image_lighting
                .map_or(&self.default_image_lighting, |lighting| &lighting.map.bind_group),
        };
        let mut draw_calls = self.encode_shadow_views(encoder, &shadows, transient_opaque);
        if self.options.depth_prepass {
            draw_calls += self.encode_depth_prepass(encoder, &draws);
        }
        let attachments = effects.as_deref().and_then(|state| state.attachments.as_ref());
        let keep_depth = effects.is_some();
        let (calls, mut triangles) =
            self.encode_scene_pass(encoder, inputs, &draws, attachments, transparency_in_scene, keep_depth);
        draw_calls += calls;

        if let Some(state) = effects {
            let (calls, drawn) =
                self.encode_effects(encoder, state, &lighting, &cameras, &draws, !transparency_in_scene)?;
            draw_calls += calls;
            triangles += drawn;
            state.previous_models = items.models.iter().copied().collect();
        }

        self.post.encode(encoder, output);
        self.release_dropped_meshes();
        self.textured.release_dropped();
        self.material_maps.release_dropped();

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
        self.diagnostics.render_size = render_size;
        Ok(())
    }

    /// Render each shadow view's culled casters, transient triangles, and terrain; returns draw calls.
    fn encode_shadow_views(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        plan: &ShadowPlan,
        transient_opaque: u32,
    ) -> usize {
        let mut draw_calls = 0;
        for (index, view) in plan.views.iter().enumerate() {
            let target = match view.target {
                ShadowTarget::Cascade(cascade) => self.shadows.cascade_layer(cascade),
                ShadowTarget::PointFace { slot, face } => self.shadows.point_layer(slot, face),
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
            pass.set_bind_group(0, self.shadows.caster(), &[ShadowMaps::caster_offset(index)]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            draw_calls += self.draw_casters(&mut pass, &view.batches);
            if view.transient && transient_opaque > 0 {
                pass.set_pipeline(&self.pipelines.caster);
                pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                pass.draw(0..transient_opaque, 0..1);
                draw_calls += 1;
            }
            draw_calls += self.terrain.draw_shadow(&mut pass, view.matrix);
        }
        draw_calls
    }

    /// Fill scene depth: terrain first, then opaque batches front to back, then transient opaque triangles. Returns
    /// draw calls.
    fn encode_depth_prepass(&self, encoder: &mut wgpu::CommandEncoder, draws: &FrameDraws<'_>) -> usize {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("depth pre-pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        let mut draw_calls = self.terrain.draw_depth(&mut pass);

        // Masked ranges alpha-test their base color, so cut-out holes stay open for whatever lies behind them.
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut masked = None;
        for batch in draws.opaque {
            if masked != Some(batch.masked) {
                pass.set_pipeline(if batch.masked { &self.pipelines.masked_depth } else { &self.pipelines.depth });
                masked = Some(batch.masked);
            }
            if batch.masked {
                pass.set_bind_group(1, self.material_maps.group(batch.material), &[]);
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

    /// The scene pass into the HDR target (and the effect attachments, when supplied): terrain, opaque batches,
    /// transient opaque triangles, the sky, and transparency when `transparency` is set. Returns draw calls and
    /// triangles. `keep_depth` stores depth for the effects and a later transparency pass.
    fn encode_scene_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &FrameInputs<'_>,
        draws: &FrameDraws<'_>,
        attachments: Option<&SceneAttachments>,
        transparency: bool,
        keep_depth: bool,
    ) -> (usize, usize) {
        let clear = inputs.clear_color;
        let mut color_attachments = vec![Some(wgpu::RenderPassColorAttachment {
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
        })];
        let targets = match attachments {
            Some(attachments) => {
                color_attachments.extend(attachments.color_attachments());
                SceneTargets::ColorAndAttachments
            }
            None => SceneTargets::Color,
        };

        // After the pre-pass, depth already holds the nearest opaque surface and only it passes the less-or-equal test.
        let depth_load = if self.options.depth_prepass { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(1.0) };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &color_attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: if keep_depth { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard },
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let pipelines = self.pipelines.opaque(targets);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, self.shadows.receiver(), &[]);
        pass.set_bind_group(2, draws.image_lighting, &[]);

        // Terrain first, as the largest occluder.
        let (mut draw_calls, mut triangles) = self.terrain.draw_lit(&mut pass, targets);

        // Opaque batches; PBR batches bind their material maps.
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let mut bound = None;
        for batch in scene_order(draws.opaque, self.options.depth_prepass) {
            if bound != Some(batch.pipeline) {
                pass.set_pipeline(match batch.pipeline {
                    OpaquePipeline::Lit => &pipelines.lit,
                    OpaquePipeline::Pbr => &pipelines.pbr,
                    OpaquePipeline::Unlit => &pipelines.unlit,
                });
                bound = Some(batch.pipeline);
            }
            if batch.pipeline == OpaquePipeline::Pbr {
                pass.set_bind_group(3, self.material_maps.group(batch.material), &[]);
            }
            triangles += self.draw_batch(&mut pass, batch);
            draw_calls += 1;
        }

        // Transient opaque triangles are lit like Lit meshes.
        if draws.transient_opaque > 0 {
            pass.set_pipeline(&pipelines.lit);
            pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
            pass.draw(0..draws.transient_opaque, 0..1);
            draw_calls += 1;
            triangles += (draws.transient_opaque / 3) as usize;
        }

        // The sky last, on the far plane, so it shades only the pixels no surface covered.
        if draws.draw_sky {
            pass.set_pipeline(&pipelines.atmosphere);
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
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            // Composite effects (temporal antialiasing) read depth after this pass.
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.post.depth_view(),
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
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
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        let (mut draw_calls, mut triangles) = (0, 0);
        for step in &draws.transparent.steps {
            match step {
                TransparentStep::Mesh(position) => {
                    let item = &draws.blends[draws.transparent.mesh_order[*position]];
                    let Some(mesh) = self.meshes.get(&item.asset.id()) else {
                        continue;
                    };
                    match item.pipeline {
                        BlendPipeline::Unlit => pass.set_pipeline(&self.pipelines.unlit_blend),
                        BlendPipeline::Pbr => {
                            // Textured runs rebind group 2, so blended PBR binds image lighting each time.
                            pass.set_pipeline(&self.pipelines.pbr_blend);
                            pass.set_bind_group(2, draws.image_lighting, &[]);
                            pass.set_bind_group(3, self.material_maps.group(item.material), &[]);
                        }
                    }
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    let instance = draws.blend_start + *position as u32;
                    pass.draw_indexed(item.indices.clone(), 0, instance..instance + 1);
                    triangles += item.indices.len() / 3;
                }
                TransparentStep::Colored(vertices) => {
                    pass.set_pipeline(&self.pipelines.unlit_blend);
                    pass.set_vertex_buffer(0, self.transient_buffer.slice(..));
                    pass.draw(vertices.clone(), 0..1);
                    triangles += vertices.len() / 3;
                }
                TransparentStep::Textured { texture, vertices } => {
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
        let jitter = if effects.runtime.options().taa.is_some() {
            let (width, height) = self.render_size();
            scene_camera_jitter(effects.runtime.frame_index(), width, height)
        } else {
            [0.0; 2]
        };
        let previous = effects.previous_camera.as_ref();
        let previous_jitter = previous.map_or([0.0; 2], |previous| previous.jitter);
        FrameCameras {
            stable: *camera,
            raster: CameraFrame { projection: jitter_projection(camera.projection, jitter), ..*camera },
            jitter,
            previous_view_projection: previous.map_or(camera.view_projection(), |previous| previous.view_projection),
            jitter_delta: [jitter[0] - previous_jitter[0], jitter[1] - previous_jitter[1]],
        }
    }

    /// Run the lighting effects over the opaque scene, draw transparency when `transparency_pending`, then run the
    /// composite effects over everything and remember this frame's camera. Returns the transparency pass's draw calls
    /// and triangles.
    fn encode_effects(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        state: &mut EffectsState,
        lighting: &FrameLighting,
        cameras: &FrameCameras,
        draws: &FrameDraws<'_>,
        transparency_pending: bool,
    ) -> Result<(usize, usize), RenderError> {
        let device = self.gpu.device();
        let queue = self.gpu.queue();
        let size = self.render_size();
        let scene_format = self.gpu.capabilities().scene_format;
        let camera = &cameras.stable;
        let view_projection = camera.view_projection();
        let raster_view_projection = cameras.raster.view_projection();
        let history = state
            .previous_camera
            .as_ref()
            .map(|previous| (previous.view_projection, previous.raster_view_projection, previous.jitter));

        // Clustered volume lighting reads the frame's selected point lights.
        let clusters = if state.runtime.options().requirements().clustered_lighting {
            let volume = state.volume.get_or_insert_with(|| VolumeLights::new(device));
            let volume_camera = VolumeCamera {
                view: camera.view,
                projection: cameras.raster.projection,
                near: camera.near,
                far: camera.far,
                width: size.0,
                height: size.1,
            };
            Some(volume.update(device, queue, &lighting.points, &volume_camera))
        } else {
            state.volume = None;
            None
        };

        let (direction, color) = match lighting.key {
            Some(light) => {
                let direction = camera.view.transform_vector3(light.direction.normalize());
                (direction.to_array(), light.radiance().rgb_array())
            }
            None => ([0.0, 1.0, 0.0], [0.0; 3]),
        };
        let attachments = state.attachments.as_ref();
        let frame = EffectsFrame {
            depth: self.post.depth_view(),
            normal: attachments.map(SceneAttachments::normal_roughness),
            velocity: attachments.map(SceneAttachments::jittered_motion),
            taa_velocity: attachments.map(SceneAttachments::motion),
            projection: cameras.raster.projection,
            inverse_projection: cameras.raster.projection.inverse(),
            inverse_view: camera.view.inverse(),
            inverse_view_projection: view_projection.inverse(),
            previous_view_projection: history.map_or(view_projection, |history| history.0),
            inverse_raster_view_projection: raster_view_projection.inverse(),
            previous_raster_view_projection: history.map_or(raster_view_projection, |history| history.1),
            near: camera.near,
            far: camera.far,
            orthographic: camera.projection.w_axis.w != 0.0,
            current_jitter: cameras.jitter,
            previous_jitter: history.map_or(cameras.jitter, |history| history.2),
            directional_direction_view: direction,
            directional_color: color,
            clusters,
            time_seconds: self.presentation_time.time_seconds as f32,
            delta_time_seconds: if history.is_some() { self.presentation_time.elapsed_seconds } else { 0.0 },
            reset_history: history.is_none(),
        };

        let scene = self.post.scene_view();
        let lit =
            state.runtime.encode(device, queue, encoder, scene, scene_format, size, &frame, EffectStage::Lighting);
        if let Err(error) = lit {
            state.reset_history();
            return Err(RenderError::InvalidFrame(error.to_string()));
        }
        let (draw_calls, triangles) =
            if transparency_pending { self.encode_transparency_pass(encoder, draws) } else { (0, 0) };
        let composited =
            state.runtime.encode(device, queue, encoder, scene, scene_format, size, &frame, EffectStage::Composite);
        if let Err(error) = composited {
            state.reset_history();
            return Err(RenderError::InvalidFrame(error.to_string()));
        }

        state.previous_camera = Some(PreviousCamera {
            eye: camera.eye,
            focus: camera.focus,
            projection: camera.projection,
            view_projection,
            raster_view_projection,
            jitter: cameras.jitter,
            time_seconds: self.presentation_time.time_seconds,
        });
        Ok((draw_calls, triangles))
    }

    /// Upload every mesh and material map the frame's items draw, the first time each appears.
    fn upload_scene_assets(&mut self, items: &SceneItems) -> Result<(), RenderError> {
        let assets = items
            .opaque
            .iter()
            .map(|item| &item.asset)
            .chain(items.casters.iter().map(|item| &item.asset))
            .chain(items.blends.iter().map(|item| &item.asset));
        for asset in assets.clone() {
            self.upload_mesh(asset);
        }
        let materials =
            assets.flat_map(|asset| asset.submeshes().iter().filter_map(|submesh| submesh.textures.as_ref()));
        for material in materials {
            self.material_maps.prepare(&self.gpu, &mut self.mipmaps, material)?;
        }
        Ok(())
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

    /// Choose this frame's shadow views, cull each view's casters, and upload the view matrices and receiver uniforms.
    ///
    /// The key light casts cascades when it is lit and set to cast. A selected point light set to cast gets six faces
    /// when its range touches a caster; a light whose reach holds no caster would see empty maps, so it is skipped.
    /// Every view draws only the casters whose bounds reach its volume, and caster instances are appended to `raws`.
    fn plan_shadows(
        &mut self,
        lighting: &FrameLighting,
        camera: &CameraFrame,
        casters: &ShadowCasters<'_>,
        raws: &mut Vec<InstanceRaw>,
    ) -> ShadowPlan {
        let mut plan = ShadowPlan::default();
        let mut uniforms = ShadowUniforms::disabled();
        let Some(options) = self.options.shadows.filter(|_| !casters.is_empty()) else {
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

        // Directional cascades. Each cascade's volume reaches toward the light by the shadow distance, so casters
        // outside every cascade (including those beyond the shadow distance) are never drawn.
        if let Some(light) = lighting.key {
            let distance = options.shadow_distance.min(camera.far).max(camera.near);
            if light.casts_shadow && light.is_lit() && distance > camera.near {
                let count = quality.cascade_count();
                let splits = practical_cascade_splits(camera.near, distance, count);
                let map_size = options.directional_map_size(self.gpu.capabilities().max_texture_dimension);
                let cascades = directional_cascades(camera, light.direction, &splits, count, distance, map_size);
                for (index, (matrix, texel)) in cascades.iter().enumerate() {
                    uniforms.cascades[index] = matrix.to_cols_array_2d();
                    uniforms.cascade_bias[index] = options.normal_bias.max(texel * CASCADE_TEXEL_BIAS);
                    plan.add_view(*matrix, ShadowTarget::Cascade(index), casters, raws);
                }
                uniforms.counts[0] = 1.0;
                uniforms.cascade_splits = splits;
                uniforms.directional =
                    [options.normal_bias, light.angular_radius, CASCADE_BLEND_FRACTION, FAR_FADE_FRACTION];
                uniforms.extents[0] = distance;
                uniforms.extents[1] = 1.0;
                plan.cascades = cascades.len();
            }
        }

        // Point-light faces, each with the casters inside that face's frustum.
        for (index, light) in lighting.points.iter().enumerate() {
            if !light.casts_shadow || !casters.touch_sphere(light.position, light.range) {
                continue;
            }
            let slot = plan.point_lights;
            uniforms.points[slot] = PointShadowRaw {
                position_range: light.position.extend(light.range).to_array(),
                parameters: [point_near_plane(light.range), POINT_SOURCE_RADIUS, options.normal_bias, 1.0],
            };
            plan.slots[index] = (slot + 1) as f32;
            for (face, matrix) in point_face_views(light.position, light.range).into_iter().enumerate() {
                plan.add_view(matrix, ShadowTarget::PointFace { slot, face }, casters, raws);
            }
            plan.point_lights += 1;
        }
        uniforms.extents[2] = plan.point_lights as f32;

        let device = self.gpu.device().clone();
        let matrices: Vec<Mat4> = plan.views.iter().map(|view| view.matrix).collect();
        self.shadows.reserve_point_lights(&device, plan.point_lights);
        self.shadows.write_casters(&device, self.gpu.queue(), &matrices);
        self.shadows.write_uniforms(self.gpu.queue(), &uniforms);
        plan
    }

    /// Pack the cameras, lights, skylight, and shadow slots into the frame uniform block.
    fn write_frame_uniforms(
        &self,
        lighting: &FrameLighting,
        shadow_slots: &[f32; MAX_POINT_LIGHTS],
        cameras: &FrameCameras,
    ) {
        let mut lights = [GpuPointLight::default(); MAX_POINT_LIGHTS];
        for ((slot, light), shadow) in lights.iter_mut().zip(&lighting.points).zip(shadow_slots) {
            slot.position_range = light.position.extend(light.range).to_array();
            slot.color = light.color.with_alpha(*shadow).to_array();
        }
        let (primary_direction, primary_color) = match lighting.key {
            Some(light) => {
                (light.direction.normalize().extend(light.intensity).to_array(), light.color.with_alpha(0.0).to_array())
            }
            None => ([0.0, 1.0, 0.0, 0.0], [0.0; 4]),
        };
        let skylight = lighting.skylight;
        let raster = &cameras.raster;
        let uniforms = FrameUniforms {
            view_projection: raster.view_projection().to_cols_array_2d(),
            view: raster.view.to_cols_array_2d(),
            eye: raster.eye.extend(self.presentation_time.time_seconds as f32).to_array(),
            lighting: [lighting.points.len() as f32, lighting.ambient, 0.0, 0.0],
            primary_direction,
            primary_color,
            skylight: skylight.color.with_alpha(skylight.intensity).to_array(),
            ground_skylight: skylight.ground_color.with_alpha(0.0).to_array(),
            stable_view_projection: cameras.stable.view_projection().to_cols_array_2d(),
            previous_view_projection: cameras.previous_view_projection.to_cols_array_2d(),
            motion: [cameras.jitter_delta[0], cameras.jitter_delta[1], 0.0, 0.0],
            lights,
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
        Self { eye: camera.eye, forward: (-backward).try_normalize().unwrap_or(Vec3::NEG_Z) }
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
        self.views.push(ShadowView { matrix, target, batches, transient });
    }
}

impl ShadowCasters<'_> {
    fn is_empty(&self) -> bool {
        self.meshes.is_empty() && self.transient.is_none() && self.terrain.is_empty()
    }

    /// Whether any caster's bounds touch the sphere at `center` with `radius`.
    fn touch_sphere(&self, center: Vec3, radius: f32) -> bool {
        let touches = |bounds: &Bounds| sphere_touches_bounds(center, radius, *bounds);
        self.meshes.iter().any(|item| touches(&item.world))
            || self.transient.as_ref().is_some_and(touches)
            || self.terrain.iter().any(touches)
    }

    /// Batches of the mesh casters whose bounds reach `view_projection`, appending their instances to `raws`.
    fn batches_for(&self, view_projection: Mat4, raws: &mut Vec<InstanceRaw>) -> Vec<Batch> {
        let mut batches = Vec::new();
        for group in self.meshes.chunk_by(|left, right| left.asset.id() == right.asset.id()) {
            let start = raws.len() as u32;
            let reaching = group.iter().filter(|item| !bounds_outside_view(item.world, view_projection));
            raws.extend(reaching.map(|item| item.raw));
            let end = raws.len() as u32;
            if end > start {
                push_asset_batches(&mut batches, OpaquePipeline::Lit, &group[0].asset, start..end);
            }
        }
        batches
    }
}

/// Cull the scene's instances against the camera and sort them into opaque, blended, and caster items.
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
        if instance.casts_shadow && !transparent {
            items.casters.push(CasterItem { asset: instance.asset.clone(), raw, world });
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

/// Route one camera-visible instance into the opaque items and/or the blended items.
fn queue_instance(
    instance: &MeshInstance,
    raw: InstanceRaw,
    world: Bounds,
    view_depth: ViewDepth,
    transparent: bool,
    items: &mut SceneItems,
) {
    let center_depth = view_depth.of(world.center());
    let blend = |pipeline: BlendPipeline, submesh: &Submesh| BlendItem {
        depth: center_depth,
        pipeline,
        asset: instance.asset.clone(),
        indices: submesh.indices.clone(),
        material: submesh.textures.as_ref().map(MaterialTextures::id),
        raw,
    };
    if transparent {
        // Transparent instances are unlit; PBR keeps its own blending.
        let pipeline = if instance.shading == Shading::Pbr { BlendPipeline::Pbr } else { BlendPipeline::Unlit };
        items.blends.extend(instance.asset.submeshes().iter().map(|submesh| blend(pipeline, submesh)));
        return;
    }

    let pipeline = match instance.shading {
        Shading::Lit => OpaquePipeline::Lit,
        Shading::Pbr => OpaquePipeline::Pbr,
        Shading::Unlit => OpaquePipeline::Unlit,
    };
    items.opaque.push(OpaqueItem { pipeline, asset: instance.asset.clone(), raw, depth: view_depth.nearest(world) });

    // PBR materials can blend part of an otherwise opaque asset.
    if instance.shading == Shading::Pbr {
        let blended = instance.asset.submeshes().iter().filter(|submesh| submesh.blend);
        items.blends.extend(blended.map(|submesh| blend(BlendPipeline::Pbr, submesh)));
    }
}

/// Group opaque items into instanced batches and append their instances to `raws`.
///
/// Items of one pipeline and asset share batches, nearest instance first, and batches are ordered by their nearest
/// instance, so depth fills front to back and the surfaces behind fail the depth test before shading.
fn batch_opaque(items: &mut [OpaqueItem], raws: &mut Vec<InstanceRaw>) -> Vec<Batch> {
    items.sort_by(|left, right| {
        (left.pipeline, left.asset.id())
            .cmp(&(right.pipeline, right.asset.id()))
            .then(left.depth.total_cmp(&right.depth))
    });
    let mut groups: Vec<&[OpaqueItem]> =
        items.chunk_by(|left, right| left.pipeline == right.pipeline && left.asset.id() == right.asset.id()).collect();
    groups.sort_by(|left, right| left[0].depth.total_cmp(&right[0].depth));

    let mut batches = Vec::new();
    for group in groups {
        let start = raws.len() as u32;
        raws.extend(group.iter().map(|item| item.raw));
        push_asset_batches(&mut batches, group[0].pipeline, &group[0].asset, start..raws.len() as u32);
    }
    batches
}

/// Record one batch per opaque submesh range of `asset`, each drawing `instances`.
fn push_asset_batches(batches: &mut Vec<Batch>, pipeline: OpaquePipeline, asset: &MeshAsset, instances: Range<u32>) {
    for (indices, submesh) in opaque_ranges(asset.submeshes()) {
        batches.push(Batch {
            pipeline,
            asset_id: asset.id(),
            indices,
            instances: instances.clone(),
            material: submesh.and_then(|submesh| submesh.textures.as_ref()).map(MaterialTextures::id),
            masked: submesh.is_some_and(|submesh| submesh.masked),
        });
    }
}

/// The scene pass's batch order. After the depth pre-pass only the nearest surface passes the depth test, so batches
/// group by pipeline (nearest first within each) to save state changes; without it they keep their front-to-back
/// order.
fn scene_order(batches: &[Batch], depth_prepass: bool) -> Vec<&Batch> {
    let mut order: Vec<&Batch> = batches.iter().collect();
    if depth_prepass {
        order.sort_by_key(|batch| batch.pipeline);
    }
    order
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

/// Pack an instance's transform and its previous frame's, tint, opacity, shadow flag, and scene roughness.
fn instance_raw(instance: &MeshInstance, model: Mat4, previous_model: Mat4) -> InstanceRaw {
    InstanceRaw {
        model: model.to_cols_array_2d(),
        tint: instance.tint.with_alpha(instance.opacity.clamp(0.0, 1.0)).to_array(),
        flags: [if instance.receives_shadow { 1.0 } else { 0.0 }, instance.scene_roughness.clamp(0.0, 1.0), 0.0, 0.0],
        previous_model: previous_model.to_cols_array_2d(),
    }
}

/// Opaque index ranges with their submeshes. An asset with no opaque submesh (drawn opaque by request) draws its whole
/// index range untextured.
fn opaque_ranges(submeshes: &[Submesh]) -> Vec<(Range<u32>, Option<&Submesh>)> {
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

/// Convert the complete opaque transient triangles to mesh vertices; incomplete trailing vertices are dropped.
fn opaque_transient_vertices(triangles: &[ColorVertex]) -> Vec<MeshVertex> {
    triangles
        .as_chunks::<3>()
        .0
        .iter()
        .flatten()
        .map(|vertex| {
            MeshVertex::new(vertex.position, [0.0; 3], LinearColor::from(vertex.color), VertexMaterial::default())
        })
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
    Some(points.fold(Bounds { min: first, max: first }, |bounds, point| Bounds {
        min: bounds.min.min(point),
        max: bounds.max.max(point),
    }))
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
