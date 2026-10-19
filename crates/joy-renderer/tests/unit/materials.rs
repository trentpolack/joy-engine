// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/materials/mod.rs`.

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
    let environment = include_str!("../../src/shaders/environment-filter.wgsl");
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
            let mut writer = glsl::Writer::new(&mut output, &module, &info, &options, &pipeline, Default::default())
                .unwrap_or_else(|error| panic!("{name} {entry_point}: {error:?}"));
            writer.write().unwrap_or_else(|error| panic!("{name} {entry_point}: {error:?}"));
            let shadow_sampler = output.contains("sampler2DArrayShadow") || output.contains("#extension");
            assert!(!shadow_sampler, "{name} {entry_point} needs a shadow sampler or extension");
        }
    }
}
