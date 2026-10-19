// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU data layouts and render pipelines shared by the renderer's passes.
//!
//! Every surface material shares one vertex layout, one instance layout, the frame uniform group, and the shadow group,
//! so material passes differ only in shading and blend state. Struct layouts here mirror the WGSL declarations exactly.
//!
//! Opaque materials come in two target layouts ([`SceneTargets`]): the HDR scene color alone, and, when scene effects
//! need them, the color plus normal-and-roughness and motion attachments written by the same draw. Opaque depth tests
//! less-or-equal and still writes, so the same pipelines draw after the depth pre-pass (where only the visible surface
//! passes) or without it.

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::frame::MAX_POINT_LIGHTS;
use crate::mesh::MeshVertex;

mod maps;
mod textured;

pub(crate) use maps::MaterialMaps;
pub(crate) use textured::TexturedPass;

/// Main depth buffer format; `Depth24Plus` is available on every backend, including WebGL 2.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

/// Shadow map format for cascades and point-light faces; sampled with comparison and plain depth samplers.
pub(crate) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Display-referred intermediate written by the resolve pass for standard dynamic range: 8-bit encoded storage keeps dark
/// gradients from banding.
pub(crate) const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Display-referred intermediate for extended-range output, which must hold encoded values above SDR white.
pub(crate) const EXTENDED_DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Format of the scene-effect attachments: view-space normals with roughness, and signed screen-space motion.
pub(crate) const ATTACHMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Attachments written beside the scene color: normal and roughness, motion, and jittered motion.
pub(crate) const SCENE_ATTACHMENT_COUNT: usize = 3;

// WGSL sources. Material shaders are the shared scene contract followed by their own entry points.
const SCENE_COMMON: &str = include_str!("../shaders/scene-common.wgsl");
const SHADOW_COMPARE_GL: &str = include_str!("../shaders/shadow-compare-gl.wgsl");
const LIT_SOURCE: &str = include_str!("../shaders/lit.wgsl");
const UNLIT_SOURCE: &str = include_str!("../shaders/unlit.wgsl");
const PBR_SOURCE: &str = include_str!("../shaders/pbr.wgsl");
const DEPTH_PREPASS_SOURCE: &str = include_str!("../shaders/depth-prepass.wgsl");
const CASTER_SOURCE: &str = include_str!("../shaders/shadow-caster.wgsl");
const MASKED_CASTER_SOURCE: &str = include_str!("../shaders/shadow-caster-masked.wgsl");
const ATMOSPHERE_SOURCE: &str = include_str!("../shaders/atmosphere.wgsl");
pub(crate) const BLOOM_SOURCE: &str = include_str!("../shaders/post-bloom.wgsl");
pub(crate) const BLUR_SOURCE: &str = include_str!("../shaders/post-blur.wgsl");
pub(crate) const RESOLVE_SOURCE: &str = include_str!("../shaders/post-resolve.wgsl");
pub(crate) const FINAL_SOURCE: &str = include_str!("../shaders/post-final.wgsl");

/// Packed point light (`PointLight` in scene-common.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct GpuPointLight {
    pub position_range: [f32; 4],
    pub color: [f32; 4],
}

/// Frame uniforms (`Frame` in scene-common.wgsl). 1,376 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct FrameUniforms {
    pub view_projection: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub lighting: [f32; 4],
    pub primary_direction: [f32; 4],
    pub primary_color: [f32; 4],
    pub skylight: [f32; 4],
    pub ground_skylight: [f32; 4],
    pub stable_view_projection: [[f32; 4]; 4],
    pub previous_view_projection: [[f32; 4]; 4],
    pub motion: [f32; 4],
    pub lights: [GpuPointLight; MAX_POINT_LIGHTS],
}

/// Column-major identity matrix.
const IDENTITY_MATRIX: [[f32; 4]; 4] =
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Per-instance vertex data (`Instance` in scene-common.wgsl). 160 bytes.
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
    /// Identity transform, white, opaque, receiving shadows, fully rough; used for transient world-space triangles.
    pub const IDENTITY: Self =
        Self { model: IDENTITY_MATRIX, tint: [1.0; 4], flags: [1.0, 1.0, 0.0, 0.0], previous_model: IDENTITY_MATRIX };
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

/// The shared scene contract for `device`. The GL backend swaps in shadow comparisons that WebGL 2 can compile.
pub(crate) fn scene_common(device: &wgpu::Device) -> String {
    if device.adapter_info().backend != wgpu::Backend::Gl {
        return SCENE_COMMON.to_owned();
    }
    const BEGIN: &str = "// SHADOW_COMPARE_BEGIN";
    const END: &str = "// SHADOW_COMPARE_END";
    match (SCENE_COMMON.find(BEGIN), SCENE_COMMON.find(END)) {
        (Some(begin), Some(end)) => {
            format!("{}{SHADOW_COMPARE_GL}{}", &SCENE_COMMON[..begin], &SCENE_COMMON[end + END.len()..])
        }
        _ => SCENE_COMMON.to_owned(),
    }
}

/// Vertex attributes of [`MeshVertex`]: locations 0 through 4, and texture coordinates at 15 (read by PBR shading and
/// masked casters only).
const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 15 => Float32x2
];

/// Instance attributes of [`InstanceRaw`], locations 5 through 14. With the mesh attributes this uses all sixteen
/// locations WebGL 2 guarantees.
pub(crate) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
    11 => Float32x4, 12 => Float32x4, 13 => Float32x4, 14 => Float32x4
];

/// Vertex buffer layouts shared by every surface pipeline: mesh vertices, then instances.
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
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
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

/// Material maps for the PBR pass and masked casters: a flags uniform, six maps, and one repeating sampler.
pub(crate) fn material_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("material maps"),
        entries: &[
            uniform_entry(0),
            texture_entry(1),
            texture_entry(2),
            texture_entry(3),
            texture_entry(4),
            texture_entry(5),
            texture_entry(6),
            sampler_entry(7),
        ],
    })
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
    /// Color target states for `scene_format`. `writes_attachments` false keeps the attachment slots in the layout but
    /// masks them, for draws (such as the sky) that share the pass without writing surface data.
    pub(crate) fn states(
        self,
        scene_format: wgpu::TextureFormat,
        writes_attachments: bool,
    ) -> Vec<Option<wgpu::ColorTargetState>> {
        let mut states = vec![Some(wgpu::ColorTargetState {
            format: scene_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        if self == Self::ColorAndAttachments {
            let write_mask = if writes_attachments { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::empty() };
            let attachment = wgpu::ColorTargetState { format: ATTACHMENT_FORMAT, blend: None, write_mask };
            states.extend(std::iter::repeat_n(Some(attachment), SCENE_ATTACHMENT_COUNT));
        }
        states
    }

    /// The material fragment entry point that writes these targets.
    pub(crate) fn fragment_entry(self) -> &'static str {
        match self {
            Self::Color => "fragmentMain",
            Self::ColorAndAttachments => "fragmentTargets",
        }
    }
}

/// The opaque scene pipelines for one target layout: the three materials and the sky.
pub(crate) struct OpaquePipelines {
    pub lit: wgpu::RenderPipeline,
    pub pbr: wgpu::RenderPipeline,
    pub unlit: wgpu::RenderPipeline,
    pub atmosphere: wgpu::RenderPipeline,
}

impl OpaquePipelines {
    fn new(
        device: &wgpu::Device,
        layouts: &SceneLayouts,
        scene_format: wgpu::TextureFormat,
        targets: SceneTargets,
    ) -> Self {
        let states = targets.states(scene_format, true);
        let entry = targets.fragment_entry();
        let material = |label: &str, source: &str, layout: &wgpu::PipelineLayout| {
            surface_pipeline(device, label, source, layout, entry, &states, opaque_depth_state())
        };
        Self {
            lit: material("lit", LIT_SOURCE, &layouts.surface),
            pbr: material("pbr", PBR_SOURCE, &layouts.pbr),
            unlit: material("unlit", UNLIT_SOURCE, &layouts.surface),
            atmosphere: atmosphere_pipeline(device, &layouts.atmosphere_group, &targets.states(scene_format, false)),
        }
    }
}

/// Pipeline layouts shared by the scene's surface pipelines.
struct SceneLayouts {
    surface: wgpu::PipelineLayout,
    pbr: wgpu::PipelineLayout,
    atmosphere_group: wgpu::BindGroupLayout,
}

/// Every pipeline and layout used by the shadow, depth pre-pass, sky, and scene passes.
pub(crate) struct ScenePipelines {
    pub frame_layout: wgpu::BindGroupLayout,
    pub shadow_layout: wgpu::BindGroupLayout,
    pub caster_layout: wgpu::BindGroupLayout,
    pub image_lighting_layout: wgpu::BindGroupLayout,
    pub material_layout: wgpu::BindGroupLayout,
    pub atmosphere_layout: wgpu::BindGroupLayout,
    /// Opaque materials and the sky writing color only.
    pub forward: OpaquePipelines,
    pub pbr_blend: wgpu::RenderPipeline,
    pub unlit_blend: wgpu::RenderPipeline,
    pub depth: wgpu::RenderPipeline,
    pub masked_depth: wgpu::RenderPipeline,
    pub caster: wgpu::RenderPipeline,
    pub masked_caster: wgpu::RenderPipeline,
    // Opaque materials and the sky with scene-effect attachments; compiled when effects first need them.
    attachments: Option<OpaquePipelines>,
    layouts: SceneLayouts,
    scene_format: wgpu::TextureFormat,
}

/// Depth biases applied when rasterizing shadow casters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CasterBias {
    pub constant: i32,
    pub slope_scale: f32,
}

impl ScenePipelines {
    /// Compile every scene pipeline for the scene color format; attachment variants compile on first use.
    pub fn new(device: &wgpu::Device, scene_format: wgpu::TextureFormat, bias: CasterBias) -> Self {
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame uniforms"),
            entries: &[uniform_entry(0)],
        });
        let shadow_layout = crate::shadows::receiver_layout(device);
        let caster_layout = crate::shadows::caster_layout(device);
        let image_lighting_layout = crate::environment::image_lighting_layout(device);
        let material_layout = material_layout(device);
        let atmosphere_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atmosphere"),
            entries: &[uniform_entry(0)],
        });

        let pipeline_layout = |label: &str, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let layouts = SceneLayouts {
            surface: pipeline_layout("surface", &[Some(&frame_layout), Some(&shadow_layout)]),
            pbr: pipeline_layout(
                "pbr surface",
                &[Some(&frame_layout), Some(&shadow_layout), Some(&image_lighting_layout), Some(&material_layout)],
            ),
            atmosphere_group: atmosphere_layout.clone(),
        };
        let forward = OpaquePipelines::new(device, &layouts, scene_format, SceneTargets::Color);

        // Blended surfaces test depth without writing it and blend straight alpha into the scene color.
        let blend_targets = [Some(wgpu::ColorTargetState {
            format: scene_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let blend = |label: &str, source: &str, layout: &wgpu::PipelineLayout| {
            surface_pipeline(device, label, source, layout, "fragmentMain", &blend_targets, depth_state(false))
        };
        let pbr_blend = blend("pbr-blend", PBR_SOURCE, &layouts.pbr);
        let unlit_blend = blend("unlit-blend", UNLIT_SOURCE, &layouts.surface);

        let depth_layout = pipeline_layout("depth pre-pass", &[Some(&frame_layout)]);
        let masked_depth_layout =
            pipeline_layout("masked depth pre-pass", &[Some(&frame_layout), Some(&material_layout)]);
        let depth = depth_prepass_pipeline(device, &depth_layout, false);
        let masked_depth = depth_prepass_pipeline(device, &masked_depth_layout, true);

        let caster = caster_pipeline(device, &caster_layout, bias);
        let masked_caster = masked_caster_pipeline(device, &caster_layout, &material_layout, bias);

        Self {
            frame_layout,
            shadow_layout,
            caster_layout,
            image_lighting_layout,
            material_layout,
            atmosphere_layout,
            forward,
            pbr_blend,
            unlit_blend,
            depth,
            masked_depth,
            caster,
            masked_caster,
            attachments: None,
            layouts,
            scene_format,
        }
    }

    /// Compile the opaque pipelines writing `targets` before a pass that draws with them opens; the attachment variants
    /// compile the first time effects need them.
    pub fn ensure_opaque(&mut self, device: &wgpu::Device, targets: SceneTargets) {
        if targets == SceneTargets::ColorAndAttachments && self.attachments.is_none() {
            self.attachments = Some(OpaquePipelines::new(device, &self.layouts, self.scene_format, targets));
        }
    }

    /// The opaque pipelines writing `targets`, after [`ensure_opaque`](Self::ensure_opaque) compiled them.
    pub fn opaque(&self, targets: SceneTargets) -> &OpaquePipelines {
        match targets {
            SceneTargets::Color => &self.forward,
            SceneTargets::ColorAndAttachments => self.attachments.as_ref().expect("ensure_opaque compiled them"),
        }
    }
}

/// Opaque depth: less-or-equal with writes, so a surface the depth pre-pass already wrote still passes, and drawing
/// without the pre-pass still resolves visibility.
fn opaque_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

/// Depth state for the scene pass's `Depth24Plus` attachment, testing less-than.
fn depth_state(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

/// Compile one surface material: the shared scene contract plus the material's entry points.
///
/// Geometry is double-sided (no culling), matching the engine's flat double-sided lighting.
pub(crate) fn surface_pipeline(
    device: &wgpu::Device,
    label: &str,
    material_source: &str,
    layout: &wgpu::PipelineLayout,
    fragment_entry: &str,
    targets: &[Option<wgpu::ColorTargetState>],
    depth: wgpu::DepthStencilState,
) -> wgpu::RenderPipeline {
    let source = format!("{}\n{material_source}", scene_common(device));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let buffers = surface_buffers();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(depth),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Depth-only pre-pass pipeline over the scene depth; the masked variant alpha-tests base color like the shadow caster.
fn depth_prepass_pipeline(device: &wgpu::Device, layout: &wgpu::PipelineLayout, masked: bool) -> wgpu::RenderPipeline {
    let label = if masked { "masked depth pre-pass" } else { "depth pre-pass" };
    let source = format!("{}\n{DEPTH_PREPASS_SOURCE}", scene_common(device));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let buffers = surface_buffers();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(if masked { "maskedVertexMain" } else { "vertexMain" }),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(opaque_depth_state()),
        multisample: wgpu::MultisampleState::default(),
        fragment: masked.then(|| wgpu::FragmentState {
            module: &module,
            entry_point: Some("maskedFragmentMain"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Depth-only caster pipeline with raster depth bias. Reads positions and instance transforms only.
fn caster_pipeline(
    device: &wgpu::Device,
    caster_layout: &wgpu::BindGroupLayout,
    bias: CasterBias,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shadow-caster"),
        source: wgpu::ShaderSource::Wgsl(CASTER_SOURCE.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shadow-caster"),
        bind_group_layouts: &[Some(caster_layout)],
        immediate_size: 0,
    });
    let buffers = surface_buffers();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow-caster"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: SHADOW_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState { constant: bias.constant, slope_scale: bias.slope_scale, clamp: 0.0 },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    })
}

/// Alpha-tested caster pipeline for masked submeshes: samples base color alpha and discards below the cutoff.
fn masked_caster_pipeline(
    device: &wgpu::Device,
    caster_layout: &wgpu::BindGroupLayout,
    material_layout: &wgpu::BindGroupLayout,
    bias: CasterBias,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shadow-caster-masked"),
        source: wgpu::ShaderSource::Wgsl(MASKED_CASTER_SOURCE.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shadow-caster-masked"),
        bind_group_layouts: &[Some(caster_layout), Some(material_layout)],
        immediate_size: 0,
    });
    let buffers = surface_buffers();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow-caster-masked"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: SHADOW_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState { constant: bias.constant, slope_scale: bias.slope_scale, clamp: 0.0 },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Full-screen sky pipeline on the far plane, drawn after opaque geometry: it tests depth less-or-equal without writing,
/// so it shades only uncovered pixels. Attachment slots in `targets` are masked.
fn atmosphere_pipeline(
    device: &wgpu::Device,
    atmosphere_layout: &wgpu::BindGroupLayout,
    targets: &[Option<wgpu::ColorTargetState>],
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("atmosphere"),
        source: wgpu::ShaderSource::Wgsl(ATMOSPHERE_SOURCE.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("atmosphere"),
        bind_group_layouts: &[Some(atmosphere_layout)],
        immediate_size: 0,
    });
    let depth = wgpu::DepthStencilState { depth_write_enabled: Some(false), ..opaque_depth_state() };
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
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Compile a full-screen post-processing pipeline with one bind group and a single color target.
pub(crate) fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    fragment_entry: &str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
