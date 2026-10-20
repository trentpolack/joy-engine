// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU data layouts and render pipelines shared by the renderer's passes.
//!
//! Every mesh pass shares one vertex layout, one instance layout, the frame uniform group, and the shadow group. Each
//! registered shading model (see [`crate::material`]) compiles into the same entry points: the forward scene pass
//! (color, or color plus scene-effect attachments), the blended scene pass, the alpha-tested depth pre-pass, and the
//! alpha-tested shadow caster; models that support deferred lighting also compile a G-buffer pass and a full-screen
//! lighting pass. A model's pipelines compile the first time a frame draws with them. Opaque surfaces without alpha
//! testing share one depth-only pre-pass pipeline and one shadow caster, whatever their model. Struct layouts here
//! mirror the WGSL declarations exactly.
//!
//! Opaque pipelines come in two target layouts ([`SceneTargets`]): the HDR scene color alone, and, when scene effects
//! need it, the color plus the motion attachment written by the same draw; G-buffer pipelines add the G-buffer after
//! either. Opaque depth tests less-or-equal and still writes, so the same pipelines draw after the depth pre-pass (where
//! only the visible surface passes) or without it. G-buffer pipelines also write their model's lighting slot into the
//! stencil buffer, which the model's lighting pass tests.

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::lighting::LightTextures;
use crate::material::{
    ModelId,
    ShadingModels,
};
use crate::mesh::MeshVertex;

mod fullscreen;
mod textured;

pub(crate) use fullscreen::{
    FULLSCREEN_SOURCE,
    FullscreenTarget,
    draw_fullscreen,
    fullscreen_pipeline,
};
pub(crate) use textured::TexturedPass;

/// Main depth buffer format, available on every backend including WebGL 2. The stencil holds each deferred surface's
/// lighting slot (zero for everything else).
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

/// Format of the G-buffer: the engine's header word and three words of model data per pixel (gbuffer-encoding.wgsl).
pub(crate) const GBUFFER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Uint;

/// Shadow map format for cascades and point-light faces; sampled with comparison and plain depth samplers.
pub(crate) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Format of the scene-effect attachment: signed screen-space motion in UV units, view depth, and coverage.
pub(crate) const ATTACHMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Attachments written beside the scene color: motion.
pub(crate) const SCENE_ATTACHMENT_COUNT: usize = 1;

/// Format of the integrated volumetric fog volume that transparent surfaces sample.
pub(crate) const FOG_VOLUME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// WGSL sources. Mesh passes compose whole files: the shared scene contract, mesh inputs, the shading-model contract,
// image-based lighting helpers, one model, then the pass's entry points.
const CONSTANTS: &str = include_str!("../shaders/constants.wgsl");
const SCENE_COMMON: &str = include_str!("../shaders/scene-common.wgsl");
const SHADOW_RECEIVER: &str = include_str!("../shaders/shadow-receiver.wgsl");
const SHADOW_COMPARE_GL: &str = include_str!("../shaders/shadow-compare-gl.wgsl");
const SCENE_VERTEX: &str = include_str!("../shaders/scene-vertex.wgsl");
const SURFACE_CONTRACT: &str = include_str!("../shaders/surface-contract.wgsl");
pub(crate) const GBUFFER_ENCODING: &str = include_str!("../shaders/gbuffer-encoding.wgsl");
const ENVIRONMENT_LIGHTING: &str = include_str!("../shaders/environment-lighting.wgsl");
const SURFACE_LIGHTING: &str = include_str!("../shaders/surface-lighting.wgsl");
const SURFACE_SCENE: &str = include_str!("../shaders/surface-scene.wgsl");
const SURFACE_GBUFFER: &str = include_str!("../shaders/surface-gbuffer.wgsl");
const DEFERRED_LIGHTING: &str = include_str!("../shaders/deferred-lighting.wgsl");
const SURFACE_DEPTH: &str = include_str!("../shaders/surface-depth.wgsl");
const SURFACE_CASTER: &str = include_str!("../shaders/surface-caster.wgsl");
const DEPTH_PREPASS_SOURCE: &str = include_str!("../shaders/depth-prepass.wgsl");
const CASTER_SOURCE: &str = include_str!("../shaders/shadow-caster.wgsl");
const ATMOSPHERE_SOURCE: &str = include_str!("../shaders/atmosphere.wgsl");
pub(crate) const BLOOM_SOURCE: &str = include_str!("../shaders/post-bloom.wgsl");
const TONE_MAPPING_SOURCE: &str = include_str!("../shaders/tone-mapping.wgsl");
const RESOLVE_SOURCE: &str = include_str!("../shaders/post-resolve.wgsl");

/// The display resolve: the tone-mapping library, then the resolve pass.
pub(crate) fn resolve_source() -> String {
    format!("{TONE_MAPPING_SOURCE}\n{RESOLVE_SOURCE}")
}

/// Frame uniforms (`Frame` in scene-common.wgsl). 496 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct FrameUniforms {
    pub view_projection: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub inverse_view_projection: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub lighting: [f32; 4],
    pub primary_direction: [f32; 4],
    pub primary_color: [f32; 4],
    pub skylight: [f32; 4],
    pub ground_skylight: [f32; 4],
    pub stable_view_projection: [[f32; 4]; 4],
    pub previous_view_projection: [[f32; 4]; 4],
    pub fog: [f32; 4],
    pub debug: [f32; 4],
    pub viewport: [f32; 4],
    pub cluster_grid: [f32; 4],
    pub cluster_depth: [f32; 4],
}

/// Column-major identity matrix.
const IDENTITY_MATRIX: [[f32; 4]; 4] =
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Per-instance vertex data (`Instance` in scene-vertex.wgsl). 160 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct InstanceRaw {
    pub model: [[f32; 4]; 4],
    pub tint: [f32; 4],
    pub flags: [f32; 4],
    /// The previous frame's model matrix, for motion vectors; equal to `model` without history.
    pub previous_model: [[f32; 4]; 4],
}

impl InstanceRaw {
    /// Identity transform, white, opaque, receiving shadows; used for transient world-space triangles.
    pub const IDENTITY: Self = Self {
        model: IDENTITY_MATRIX,
        tint: [1.0; 4],
        flags: [1.0, 0.0, 0.0, 0.0],
        previous_model: IDENTITY_MATRIX,
    };
}

/// Sky uniforms (`Atmosphere` in atmosphere.wgsl). 352 bytes; the field order and layout mirror the WGSL struct.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct AtmosphereUniforms {
    pub inverse_view_projection: [[f32; 4]; 4],
    pub camera_position: [f32; 4],
    pub viewport: [f32; 4],
    pub environment: [[f32; 4]; 16],
}

/// The shared constants, scene contract, and shadow receivers for `device`. The GL backend swaps in shadow comparisons
/// that WebGL 2 can compile.
pub(crate) fn scene_common(device: &wgpu::Device) -> String {
    scene_common_for(device.adapter_info().backend == wgpu::Backend::Gl)
}

/// The shared scene sources, with WebGL 2 shadow comparisons when `gl`.
pub(crate) fn scene_common_for(gl: bool) -> String {
    let receiver = if gl { gl_shadow_receiver() } else { SHADOW_RECEIVER.to_owned() };
    format!("{CONSTANTS}\n{SCENE_COMMON}\n{receiver}")
}

/// The shadow receivers with their comparisons replaced by the WebGL 2 variant.
fn gl_shadow_receiver() -> String {
    const BEGIN: &str = "// SHADOW_COMPARE_BEGIN";
    const END: &str = "// SHADOW_COMPARE_END";
    match (SHADOW_RECEIVER.find(BEGIN), SHADOW_RECEIVER.find(END)) {
        (Some(begin), Some(end)) => {
            format!("{}{SHADOW_COMPARE_GL}{}", &SHADOW_RECEIVER[..begin], &SHADOW_RECEIVER[end + END.len()..])
        }
        _ => SHADOW_RECEIVER.to_owned(),
    }
}

/// The shading-model contract and everything a model file may call, composed before every model.
fn model_prelude() -> String {
    format!("{SCENE_VERTEX}\n{SURFACE_CONTRACT}\n{GBUFFER_ENCODING}\n{ENVIRONMENT_LIGHTING}")
}

/// The complete forward scene-pass module source for a shading model's WGSL: every entry point a model draws with in
/// forward shading. Also used to validate games' models before they register.
pub(crate) fn scene_module_source(common: &str, model_source: &str) -> String {
    format!("{common}\n{}\n{model_source}\n{SURFACE_LIGHTING}\n{SURFACE_SCENE}", model_prelude())
}

/// The G-buffer pass module for a model that supports deferred lighting.
pub(crate) fn gbuffer_module_source(common: &str, model_source: &str) -> String {
    format!("{}\n{SURFACE_GBUFFER}", scene_module_source(common, model_source))
}

/// The full-screen deferred lighting module for a model that supports deferred lighting.
pub(crate) fn lighting_module_source(common: &str, model_source: &str) -> String {
    format!("{common}\n{}\n{model_source}\n{SURFACE_LIGHTING}\n{DEFERRED_LIGHTING}", model_prelude())
}

/// The alpha-tested depth pre-pass module for a model.
fn depth_module_source(common: &str, model_source: &str) -> String {
    format!("{common}\n{}\n{model_source}\n{SURFACE_DEPTH}", model_prelude())
}

/// The alpha-tested shadow caster module for a model. It has no frame or shadow groups, only the caster's light view.
fn caster_module_source(model_source: &str) -> String {
    format!("{CONSTANTS}\n{SURFACE_CASTER}\n{}\n{model_source}", model_prelude())
}

/// Vertex attributes of [`MeshVertex`]: position, normal, color, and texture coordinates at locations 0 through 3.
const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];

/// Instance attributes of [`InstanceRaw`], locations 5 through 14. With the mesh attributes this stays within the
/// sixteen locations WebGL 2 guarantees.
pub(crate) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
    11 => Float32x4, 12 => Float32x4, 13 => Float32x4, 14 => Float32x4
];

/// Vertex buffer layouts shared by every mesh pipeline: mesh vertices, then instances.
fn surface_buffers() -> [Option<wgpu::VertexBufferLayout<'static>>; 2] {
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &MESH_ATTRIBUTES,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        }),
    ]
}

/// Uniform buffer binding visible to both stages.
pub(crate) fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Filterable 2D float texture binding for the fragment stage.
pub(crate) fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float {
                filterable: true,
            },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// Filterable 3D float texture binding for the fragment stage.
pub(crate) fn volume_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float {
                filterable: true,
            },
            view_dimension: wgpu::TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    }
}

/// Filtering sampler binding for the fragment stage.
pub(crate) fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// Which color targets an opaque scene pipeline writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SceneTargets {
    /// The HDR scene color only.
    Color,
    /// The HDR scene color plus the [`SCENE_ATTACHMENT_COUNT`] scene-effect attachments.
    ColorAndAttachments,
}

impl SceneTargets {
    /// Color target states for `scene_format`. Every target shares one blend state and write mask, because WebGL 2
    /// cannot blend or mask targets independently; draws that carry no surface data (the sky) write the attachments'
    /// cleared values instead of masking them.
    pub(crate) fn states(self, scene_format: wgpu::TextureFormat) -> Vec<Option<wgpu::ColorTargetState>> {
        let mut states = vec![Some(wgpu::ColorTargetState {
            format: scene_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        if self == Self::ColorAndAttachments {
            let attachment = wgpu::ColorTargetState {
                format: ATTACHMENT_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            };
            states.extend(std::iter::repeat_n(Some(attachment), SCENE_ATTACHMENT_COUNT));
        }
        states
    }

    /// The scene fragment entry point that writes these targets.
    pub(crate) fn fragment_entry(self) -> &'static str {
        match self {
            Self::Color => "fragmentMain",
            Self::ColorAndAttachments => "fragmentTargets",
        }
    }

    /// Color target states for the G-buffer pass: these targets followed by the G-buffer.
    pub(crate) fn gbuffer_states(self, scene_format: wgpu::TextureFormat) -> Vec<Option<wgpu::ColorTargetState>> {
        let mut states = self.states(scene_format);
        states.push(Some(wgpu::ColorTargetState {
            format: GBUFFER_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }));
        states
    }

    /// The G-buffer fragment entry point that writes these targets and the G-buffer (surface-gbuffer.wgsl and
    /// terrain-scene.wgsl).
    pub(crate) fn gbuffer_entry(self) -> &'static str {
        match self {
            Self::Color => "fragmentGBuffer",
            Self::ColorAndAttachments => "fragmentGBufferTargets",
        }
    }
}

/// One pass a shading model draws in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelPass {
    /// Opaque or masked surfaces in the forward scene pass.
    Scene(SceneTargets),
    /// Opaque or masked surfaces of a deferred model in the G-buffer pass.
    GBuffer(SceneTargets),
    /// A deferred model's full-screen lighting pass.
    Lighting,
    /// Blended surfaces and transparent instances.
    Blend,
    /// The alpha-tested depth pre-pass.
    MaskedDepth,
    /// The alpha-tested shadow caster.
    MaskedCaster,
}

/// One model's pipelines, compiled on first use.
#[derive(Default)]
struct ModelPipelines {
    scene: Option<wgpu::RenderPipeline>,
    scene_targets: Option<wgpu::RenderPipeline>,
    gbuffer: Option<wgpu::RenderPipeline>,
    gbuffer_targets: Option<wgpu::RenderPipeline>,
    lighting: Option<wgpu::RenderPipeline>,
    blend: Option<wgpu::RenderPipeline>,
    masked_depth: Option<wgpu::RenderPipeline>,
    masked_caster: Option<wgpu::RenderPipeline>,
}

impl ModelPipelines {
    fn slot(&mut self, pass: ModelPass) -> &mut Option<wgpu::RenderPipeline> {
        match pass {
            ModelPass::Scene(SceneTargets::Color) => &mut self.scene,
            ModelPass::Scene(SceneTargets::ColorAndAttachments) => &mut self.scene_targets,
            ModelPass::GBuffer(SceneTargets::Color) => &mut self.gbuffer,
            ModelPass::GBuffer(SceneTargets::ColorAndAttachments) => &mut self.gbuffer_targets,
            ModelPass::Lighting => &mut self.lighting,
            ModelPass::Blend => &mut self.blend,
            ModelPass::MaskedDepth => &mut self.masked_depth,
            ModelPass::MaskedCaster => &mut self.masked_caster,
        }
    }

    fn get(&self, pass: ModelPass) -> Option<&wgpu::RenderPipeline> {
        match pass {
            ModelPass::Scene(SceneTargets::Color) => self.scene.as_ref(),
            ModelPass::Scene(SceneTargets::ColorAndAttachments) => self.scene_targets.as_ref(),
            ModelPass::GBuffer(SceneTargets::Color) => self.gbuffer.as_ref(),
            ModelPass::GBuffer(SceneTargets::ColorAndAttachments) => self.gbuffer_targets.as_ref(),
            ModelPass::Lighting => self.lighting.as_ref(),
            ModelPass::Blend => self.blend.as_ref(),
            ModelPass::MaskedDepth => self.masked_depth.as_ref(),
            ModelPass::MaskedCaster => self.masked_caster.as_ref(),
        }
    }
}

/// Depth biases applied when rasterizing shadow casters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CasterBias {
    pub constant: i32,
    pub slope_scale: f32,
}

/// Every pipeline and layout used by the shadow, depth pre-pass, sky, and scene passes.
pub(crate) struct ScenePipelines {
    pub frame_layout: wgpu::BindGroupLayout,
    /// The frame group plus the scene depth and G-buffer, for deferred lighting passes.
    pub deferred_frame_layout: wgpu::BindGroupLayout,
    pub shadow_layout: wgpu::BindGroupLayout,
    pub caster_layout: wgpu::BindGroupLayout,
    pub image_lighting_layout: wgpu::BindGroupLayout,
    pub atmosphere_layout: wgpu::BindGroupLayout,
    /// Depth-only pre-pass and shadow caster for opaque surfaces of any model.
    pub depth: wgpu::RenderPipeline,
    pub caster: wgpu::RenderPipeline,
    /// The sky for each scene target layout; the attachment variant compiles when effects first need it.
    atmosphere: wgpu::RenderPipeline,
    atmosphere_targets: Option<wgpu::RenderPipeline>,
    // Each registered model's pipelines, by registry index.
    models: Vec<ModelPipelines>,
    scene_format: wgpu::TextureFormat,
    bias: CasterBias,
}

impl ScenePipelines {
    /// Compile the shared pipelines for the scene color format; model pipelines compile on first use. `compute` lets
    /// compute shaders read the shadow receiver group.
    pub fn new(device: &wgpu::Device, scene_format: wgpu::TextureFormat, bias: CasterBias, compute: bool) -> Self {
        let mut frame_entries = vec![uniform_entry(0), volume_entry(1), sampler_entry(2)];
        frame_entries.extend(LightTextures::layout_entries(3, wgpu::ShaderStages::FRAGMENT));
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame uniforms"),
            entries: &frame_entries,
        });
        frame_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        frame_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 7,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        let deferred_frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deferred frame"),
            entries: &frame_entries,
        });
        let shadow_layout = crate::shadows::receiver_layout(device, compute);
        let caster_layout = crate::shadows::caster_layout(device);
        let image_lighting_layout = crate::environment::image_lighting_layout(device);
        let atmosphere_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atmosphere"),
            entries: &[uniform_entry(0)],
        });
        let depth = depth_prepass_pipeline(device, &frame_layout);
        let caster = caster_pipeline(device, &caster_layout, bias);
        let atmosphere = atmosphere_pipeline(device, &atmosphere_layout, SceneTargets::Color, scene_format);
        Self {
            frame_layout,
            deferred_frame_layout,
            shadow_layout,
            caster_layout,
            image_lighting_layout,
            atmosphere_layout,
            depth,
            caster,
            atmosphere,
            atmosphere_targets: None,
            models: Vec::new(),
            scene_format,
            bias,
        }
    }

    /// Compile the sky writing `targets` before a pass that draws with it opens.
    pub fn ensure_atmosphere(&mut self, device: &wgpu::Device, targets: SceneTargets) {
        if targets == SceneTargets::ColorAndAttachments && self.atmosphere_targets.is_none() {
            self.atmosphere_targets =
                Some(atmosphere_pipeline(device, &self.atmosphere_layout, targets, self.scene_format));
        }
    }

    /// The sky writing `targets`, after [`ensure_atmosphere`](Self::ensure_atmosphere).
    pub fn atmosphere(&self, targets: SceneTargets) -> &wgpu::RenderPipeline {
        match targets {
            SceneTargets::Color => &self.atmosphere,
            SceneTargets::ColorAndAttachments => {
                self.atmosphere_targets.as_ref().expect("ensure_atmosphere compiled the attachment sky")
            }
        }
    }

    /// Compile model `id`'s pipeline for `pass` before a pass that draws with it opens.
    pub fn ensure_model(&mut self, device: &wgpu::Device, models: &ShadingModels, id: ModelId, pass: ModelPass) {
        while self.models.len() < models.len() {
            self.models.push(ModelPipelines::default());
        }
        let slot = self.models[id].slot(pass);
        if slot.is_some() {
            return;
        }
        let model = models.get(id);
        let name = &model.definition.name;
        let source = &model.definition.source;
        *slot = Some(match pass {
            ModelPass::Scene(targets) => {
                let layouts = [
                    Some(&self.frame_layout),
                    Some(&self.shadow_layout),
                    Some(&self.image_lighting_layout),
                    Some(&model.layout),
                ];
                let module = scene_module_source(&scene_common(device), source);
                let states = targets.states(self.scene_format);
                let shape = PipelineShape {
                    label: &format!("{name} scene"),
                    source: &module,
                    layouts: &layouts,
                    vertex_entry: "vertexMain",
                    fragment_entry: Some(targets.fragment_entry()),
                    targets: &states,
                    depth: opaque_depth_state(DEPTH_FORMAT, Default::default()),
                };
                mesh_pipeline(device, &shape)
            }
            ModelPass::GBuffer(targets) => {
                let layouts = [
                    Some(&self.frame_layout),
                    Some(&self.shadow_layout),
                    Some(&self.image_lighting_layout),
                    Some(&model.layout),
                ];
                let module = gbuffer_module_source(&scene_common(device), source);
                let states = targets.gbuffer_states(self.scene_format);
                let shape = PipelineShape {
                    label: &format!("{name} g-buffer"),
                    source: &module,
                    layouts: &layouts,
                    vertex_entry: "vertexMain",
                    fragment_entry: Some(targets.gbuffer_entry()),
                    targets: &states,
                    depth: gbuffer_depth_state(),
                };
                mesh_pipeline(device, &shape)
            }
            ModelPass::Lighting => {
                let module = lighting_module_source(&scene_common(device), source);
                lighting_pipeline(
                    device,
                    &format!("{name} lighting"),
                    &module,
                    &[Some(&self.deferred_frame_layout), Some(&self.shadow_layout)],
                    self.scene_format,
                )
            }
            ModelPass::Blend => {
                let layouts = [
                    Some(&self.frame_layout),
                    Some(&self.shadow_layout),
                    Some(&self.image_lighting_layout),
                    Some(&model.layout),
                ];
                let module = scene_module_source(&scene_common(device), source);
                // Blended surfaces test depth without writing it and blend straight alpha into the scene color.
                let states = [Some(wgpu::ColorTargetState {
                    format: self.scene_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })];
                let shape = PipelineShape {
                    label: &format!("{name} blend"),
                    source: &module,
                    layouts: &layouts,
                    vertex_entry: "vertexMain",
                    fragment_entry: Some("fragmentBlend"),
                    targets: &states,
                    depth: wgpu::DepthStencilState {
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        ..opaque_depth_state(DEPTH_FORMAT, Default::default())
                    },
                };
                mesh_pipeline(device, &shape)
            }
            ModelPass::MaskedDepth => {
                let layouts = [Some(&self.frame_layout), None, None, Some(&model.layout)];
                let module = depth_module_source(&scene_common(device), source);
                let shape = PipelineShape {
                    label: &format!("{name} masked depth"),
                    source: &module,
                    layouts: &layouts,
                    vertex_entry: "maskedVertexMain",
                    fragment_entry: Some("maskedFragmentMain"),
                    targets: &[],
                    depth: opaque_depth_state(DEPTH_FORMAT, Default::default()),
                };
                mesh_pipeline(device, &shape)
            }
            ModelPass::MaskedCaster => {
                let layouts = [Some(&self.caster_layout), None, None, Some(&model.layout)];
                let module = caster_module_source(source);
                let shape = PipelineShape {
                    label: &format!("{name} masked caster"),
                    source: &module,
                    layouts: &layouts,
                    vertex_entry: "vertexMain",
                    fragment_entry: Some("fragmentMain"),
                    targets: &[],
                    depth: opaque_depth_state(SHADOW_FORMAT, caster_bias(self.bias)),
                };
                mesh_pipeline(device, &shape)
            }
        });
    }

    /// Model `id`'s pipeline for `pass`, after [`ensure_model`](Self::ensure_model).
    pub fn model(&self, id: ModelId, pass: ModelPass) -> &wgpu::RenderPipeline {
        self.models.get(id).and_then(|pipelines| pipelines.get(pass)).expect("ensure_model compiled the pipeline")
    }
}

/// Opaque depth: less-or-equal with writes, so a surface the depth pre-pass already wrote still passes, and drawing
/// without the pre-pass still resolves visibility.
fn opaque_depth_state(format: wgpu::TextureFormat, bias: wgpu::DepthBiasState) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias,
    }
}

/// G-buffer depth: opaque depth, plus the draw's stencil reference (its model's lighting slot) written wherever a
/// surface passes the depth test.
pub(crate) fn gbuffer_depth_state() -> wgpu::DepthStencilState {
    let write_slot = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Always,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Replace,
    };
    wgpu::DepthStencilState {
        stencil: wgpu::StencilState {
            front: write_slot,
            back: write_slot,
            read_mask: 0xff,
            write_mask: 0xff,
        },
        ..opaque_depth_state(DEPTH_FORMAT, Default::default())
    }
}

/// Compile a model's full-screen lighting pipeline: it adds into the scene color wherever the stencil holds the
/// draw's reference, reading depth and stencil without writing them.
fn lighting_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    layouts: &[Option<&wgpu::BindGroupLayout>],
    scene_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: layouts,
        immediate_size: 0,
    });
    let match_slot = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Keep,
    };
    let additive = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("lightingVertexMain"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: match_slot,
                back: match_slot,
                read_mask: 0xff,
                write_mask: 0,
            },
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("lightingFragmentMain"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: scene_format,
                blend: Some(additive),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The raster depth bias of shadow casters.
fn caster_bias(bias: CasterBias) -> wgpu::DepthBiasState {
    wgpu::DepthBiasState {
        constant: bias.constant,
        slope_scale: bias.slope_scale,
        clamp: 0.0,
    }
}

/// Everything that varies between mesh pipelines.
struct PipelineShape<'a> {
    label: &'a str,
    source: &'a str,
    layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
    vertex_entry: &'a str,
    fragment_entry: Option<&'a str>,
    targets: &'a [Option<wgpu::ColorTargetState>],
    depth: wgpu::DepthStencilState,
}

/// Compile one mesh pipeline over the shared vertex and instance layouts. Geometry is double-sided (no culling),
/// matching the engine's double-sided lighting.
fn mesh_pipeline(device: &wgpu::Device, shape: &PipelineShape<'_>) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(shape.label),
        source: wgpu::ShaderSource::Wgsl(shape.source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(shape.label),
        bind_group_layouts: shape.layouts,
        immediate_size: 0,
    });
    let buffers = surface_buffers();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(shape.label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(shape.vertex_entry),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(shape.depth.clone()),
        multisample: wgpu::MultisampleState::default(),
        fragment: shape.fragment_entry.map(|entry| wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: shape.targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Depth-only pre-pass pipeline over the scene depth for opaque surfaces without alpha testing.
fn depth_prepass_pipeline(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let source = format!("{}\n{SCENE_VERTEX}\n{DEPTH_PREPASS_SOURCE}", scene_common(device));
    let shape = PipelineShape {
        label: "depth pre-pass",
        source: &source,
        layouts: &[Some(frame_layout)],
        vertex_entry: "vertexMain",
        fragment_entry: None,
        targets: &[],
        depth: opaque_depth_state(DEPTH_FORMAT, Default::default()),
    };
    mesh_pipeline(device, &shape)
}

/// Depth-only caster pipeline with raster depth bias. Reads positions and instance transforms only.
fn caster_pipeline(
    device: &wgpu::Device,
    caster_layout: &wgpu::BindGroupLayout,
    bias: CasterBias,
) -> wgpu::RenderPipeline {
    let shape = PipelineShape {
        label: "shadow-caster",
        source: CASTER_SOURCE,
        layouts: &[Some(caster_layout)],
        vertex_entry: "vertexMain",
        fragment_entry: None,
        targets: &[],
        depth: opaque_depth_state(SHADOW_FORMAT, caster_bias(bias)),
    };
    mesh_pipeline(device, &shape)
}

/// Full-screen sky pipeline on the far plane, drawn after opaque geometry: it tests depth less-or-equal without writing,
/// so it shades only uncovered pixels. Attachment slots in `targets` are masked.
fn atmosphere_pipeline(
    device: &wgpu::Device,
    atmosphere_layout: &wgpu::BindGroupLayout,
    targets: SceneTargets,
    scene_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let states = targets.states(scene_format);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("atmosphere"),
        source: wgpu::ShaderSource::Wgsl(ATMOSPHERE_SOURCE.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("atmosphere"),
        bind_group_layouts: &[Some(atmosphere_layout)],
        immediate_size: 0,
    });
    let depth = wgpu::DepthStencilState {
        depth_write_enabled: Some(false),
        ..opaque_depth_state(DEPTH_FORMAT, Default::default())
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("atmosphere"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(depth),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(targets.fragment_entry()),
            compilation_options: Default::default(),
            targets: &states,
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
