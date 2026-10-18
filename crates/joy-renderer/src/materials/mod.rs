// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU data layouts and render pipelines shared by the renderer's passes.
//!
//! Every surface material shares one vertex layout, one instance layout, the frame uniform group, and the shadow group,
//! so material passes differ only in shading and blend state. Struct layouts here mirror the WGSL declarations exactly.

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::frame::MAX_POINT_LIGHTS;
use crate::mesh::MeshVertex;

mod maps;
mod textured;

pub(crate) use maps::MaterialMaps;
pub(crate) use textured::{
    PlannedDraw,
    TexturedPass,
};

/// Main depth buffer format (matches the TypeScript renderer's `depth24plus`).
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

/// Shadow map format for cascades and point-light faces; sampled with comparison and plain depth samplers.
pub(crate) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Display-referred intermediate written by the resolve pass.
pub(crate) const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

// WGSL sources. Material shaders are the shared scene contract followed by their own entry points.
const SCENE_COMMON: &str = include_str!("../shaders/scene-common.wgsl");
const SHADOW_COMPARE_GL: &str = include_str!("../shaders/shadow-compare-gl.wgsl");
const LIT_SOURCE: &str = include_str!("../shaders/lit.wgsl");
const UNLIT_SOURCE: &str = include_str!("../shaders/unlit.wgsl");
const PBR_SOURCE: &str = include_str!("../shaders/pbr.wgsl");
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

/// Frame uniforms (`Frame` in scene-common.wgsl). 1,232 bytes.
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
    pub lights: [GpuPointLight; MAX_POINT_LIGHTS],
}

/// Per-instance vertex data (`Instance` in scene-common.wgsl). 96 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct InstanceRaw {
    pub model: [[f32; 4]; 4],
    pub tint: [f32; 4],
    pub flags: [f32; 4],
}

impl InstanceRaw {
    /// Identity transform, white, opaque, receiving shadows, fully rough; used for transient world-space triangles.
    pub const IDENTITY: Self = Self {
        model: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        tint: [1.0; 4],
        flags: [1.0, 1.0, 0.0, 0.0],
    };
}

/// Sky uniforms (`Atmosphere` in atmosphere.wgsl). 352 bytes, identical to the TypeScript block.
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

/// Instance attributes of [`InstanceRaw`], locations 5 through 10.
pub(crate) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4
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

/// Every pipeline and layout used by the shadow, sky, and scene passes.
pub(crate) struct ScenePipelines {
    pub frame_layout: wgpu::BindGroupLayout,
    pub shadow_layout: wgpu::BindGroupLayout,
    pub caster_layout: wgpu::BindGroupLayout,
    pub image_lighting_layout: wgpu::BindGroupLayout,
    pub material_layout: wgpu::BindGroupLayout,
    pub atmosphere_layout: wgpu::BindGroupLayout,
    pub lit: wgpu::RenderPipeline,
    pub pbr: wgpu::RenderPipeline,
    pub unlit: wgpu::RenderPipeline,
    pub pbr_blend: wgpu::RenderPipeline,
    pub unlit_blend: wgpu::RenderPipeline,
    pub caster: wgpu::RenderPipeline,
    pub masked_caster: wgpu::RenderPipeline,
    pub atmosphere: wgpu::RenderPipeline,
}

/// Depth biases applied when rasterizing shadow casters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CasterBias {
    pub constant: i32,
    pub slope_scale: f32,
}

impl ScenePipelines {
    /// Compile every scene pipeline for the scene color format.
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

        let surface_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("surface"),
            bind_group_layouts: &[Some(&frame_layout), Some(&shadow_layout)],
            immediate_size: 0,
        });
        let pbr_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr surface"),
            bind_group_layouts: &[
                Some(&frame_layout),
                Some(&shadow_layout),
                Some(&image_lighting_layout),
                Some(&material_layout),
            ],
            immediate_size: 0,
        });

        // Opaque surfaces write depth; blended surfaces test it without writing.
        let opaque_depth = depth_state(true, wgpu::CompareFunction::Less);
        let blend_depth = depth_state(false, wgpu::CompareFunction::Less);
        let straight_alpha = Some(wgpu::BlendState::ALPHA_BLENDING);

        let lit =
            surface_pipeline(device, "lit", LIT_SOURCE, &surface_layout, scene_format, None, opaque_depth.clone());
        let pbr = surface_pipeline(device, "pbr", PBR_SOURCE, &pbr_layout, scene_format, None, opaque_depth.clone());
        let unlit = surface_pipeline(device, "unlit", UNLIT_SOURCE, &surface_layout, scene_format, None, opaque_depth);
        let pbr_blend = surface_pipeline(
            device,
            "pbr-blend",
            PBR_SOURCE,
            &pbr_layout,
            scene_format,
            straight_alpha,
            blend_depth.clone(),
        );
        let unlit_blend = surface_pipeline(
            device,
            "unlit-blend",
            UNLIT_SOURCE,
            &surface_layout,
            scene_format,
            straight_alpha,
            blend_depth,
        );

        let caster = caster_pipeline(device, &caster_layout, bias);
        let masked_caster = masked_caster_pipeline(device, &caster_layout, &material_layout, bias);
        let atmosphere = atmosphere_pipeline(device, &atmosphere_layout, scene_format);

        Self {
            frame_layout,
            shadow_layout,
            caster_layout,
            image_lighting_layout,
            material_layout,
            atmosphere_layout,
            lit,
            pbr,
            unlit,
            pbr_blend,
            unlit_blend,
            caster,
            masked_caster,
            atmosphere,
        }
    }
}

/// Depth state for the scene pass's `Depth24Plus` attachment.
fn depth_state(write: bool, compare: wgpu::CompareFunction) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
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
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
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
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
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

/// Full-screen sky pipeline drawn first in the scene pass; depth is attached but neither tested nor written.
fn atmosphere_pipeline(
    device: &wgpu::Device,
    atmosphere_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
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
        depth_stencil: Some(depth_state(false, wgpu::CompareFunction::Always)),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
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
mod tests {
    use super::*;

    /// The GL variant of every surface material, the masked caster, and the environment filter translate to WebGL 2
    /// GLSL without unsupported extensions.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gl_shadow_variant_translates_for_webgl() {
        use wgpu::naga::back::glsl;
        let (begin, end) = (SCENE_COMMON.find("// SHADOW_COMPARE_BEGIN"), SCENE_COMMON.find("// SHADOW_COMPARE_END"));
        let (Some(begin), Some(end)) = (begin, end) else {
            panic!("scene-common.wgsl lost its shadow comparison markers");
        };
        let common = format!("{}{SHADOW_COMPARE_GL}{}", &SCENE_COMMON[..begin], &SCENE_COMMON[end + 22..]);
        let environment = include_str!("../shaders/environment-filter.wgsl");
        let sources = [
            ("lit", format!("{common}\n{LIT_SOURCE}")),
            ("pbr", format!("{common}\n{PBR_SOURCE}")),
            ("unlit", format!("{common}\n{UNLIT_SOURCE}")),
            ("masked caster", MASKED_CASTER_SOURCE.to_owned()),
            ("environment filter", environment.to_owned()),
        ];
        for (name, source) in sources {
            let module = wgpu::naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
            let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            // Browsers may report the shadow-LOD extension that WebGL 2 shaders then fail to enable, so translate as if
            // it were available and require that no shadow sampler is used at all.
            let options = glsl::Options {
                version: glsl::Version::Embedded { version: 300, is_webgl: true },
                writer_flags: glsl::WriterFlags::TEXTURE_SHADOW_LOD,
                ..Default::default()
            };
            for (stage, entry_point) in
                [(wgpu::naga::ShaderStage::Vertex, "vertexMain"), (wgpu::naga::ShaderStage::Fragment, "fragmentMain")]
            {
                let pipeline =
                    glsl::PipelineOptions { shader_stage: stage, entry_point: entry_point.into(), multiview: None };
                let mut output = String::new();
                let mut writer =
                    glsl::Writer::new(&mut output, &module, &info, &options, &pipeline, Default::default())
                        .unwrap_or_else(|error| panic!("{name} {entry_point}: {error:?}"));
                writer.write().unwrap_or_else(|error| panic!("{name} {entry_point}: {error:?}"));
                let shadow_sampler = output.contains("sampler2DArrayShadow") || output.contains("#extension");
                assert!(!shadow_sampler, "{name} {entry_point} needs a shadow sampler or extension");
            }
        }
    }
}
