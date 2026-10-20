// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/pipelines/mod.rs`.

use super::*;

/// The built-in models' sources, as the registry composes them.
fn builtin_models() -> [(&'static str, &'static str); 3] {
    [
        ("lit", include_str!("../shaders/models/lit.wgsl")),
        ("layered", include_str!("../shaders/models/layered.wgsl")),
        ("unlit", include_str!("../shaders/models/unlit.wgsl")),
    ]
}

/// The shadow maps' plain-float views in shadow-receiver.wgsl.
const SHADOW_DEPTH_VIEWS: [&str; 2] = ["cascadeDepths", "pointShadowDepths"];

/// Every built-in model composes into valid scene, depth pre-pass, and shadow caster modules, and the GL variants
/// translate to WebGL 2 GLSL without shadow samplers or extensions or depth reads WebGL 2 would leave incomplete.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn every_model_pass_validates_and_translates_for_webgl() {
    use wgpu::naga::back::glsl;
    let common = scene_common_for(true);
    let mut modules = Vec::new();
    for (name, source) in builtin_models() {
        let scene = scene_module_source(&common, source);
        modules.push((
            format!("{name} scene"),
            scene.clone(),
            "vertexMain",
            vec!["fragmentMain", "fragmentBlend", "fragmentTargets"],
        ));
        modules.push((
            format!("{name} depth"),
            depth_module_source(&common, source),
            "maskedVertexMain",
            vec!["maskedFragmentMain"],
        ));
        modules.push((format!("{name} caster"), caster_module_source(source), "vertexMain", vec!["fragmentMain"]));
    }
    modules.push(("sky".into(), ATMOSPHERE_SOURCE.to_owned(), "vertexMain", vec!["fragmentMain", "fragmentTargets"]));

    for (name, source, vertex_entry, fragment_entries) in modules {
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(&source)));
        let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        // Browsers may report the shadow-LOD extension that WebGL 2 shaders then fail to enable, so translate as if
        // it were available and require that no shadow sampler is used at all.
        let options = glsl::Options {
            version: glsl::Version::Embedded {
                version: 300,
                is_webgl: true,
            },
            writer_flags: glsl::WriterFlags::TEXTURE_SHADOW_LOD,
            ..Default::default()
        };
        let stages = std::iter::once((wgpu::naga::ShaderStage::Vertex, vertex_entry))
            .chain(fragment_entries.into_iter().map(|entry| (wgpu::naga::ShaderStage::Fragment, entry)));
        for (stage, entry_point) in stages {
            let pipeline = glsl::PipelineOptions {
                shader_stage: stage,
                entry_point: entry_point.into(),
                multiview: None,
            };
            let mut output = String::new();
            let mut writer = glsl::Writer::new(&mut output, &module, &info, &options, &pipeline, Default::default())
                .unwrap_or_else(|error| panic!("{name} {entry_point}: {error:?}"));
            let reflection = writer.write().unwrap_or_else(|error| panic!("{name} {entry_point}: {error:?}"));
            let shadow_sampler = output.contains("sampler2DShadow")
                || output.contains("sampler2DArrayShadow")
                || output.contains("#extension");
            assert!(!shadow_sampler, "{name} {entry_point} needs a shadow sampler or extension");

            // WebGL 2 returns zero from depth textures read without a nearest sampler or comparison, so the shadow
            // maps' plain views must always be paired with one.
            for mapping in reflection.texture_mapping.values() {
                let texture = module.global_variables[mapping.texture].name.as_deref().unwrap_or_default();
                if SHADOW_DEPTH_VIEWS.contains(&texture) {
                    assert!(mapping.sampler.is_some(), "{name} {entry_point} reads {texture} without a sampler");
                }
            }
        }
    }
}

/// WebGL 2 cannot blend or mask color targets independently, so every target of a scene pass shares one state; the sky
/// writes the attachment rather than masking it.
#[test]
fn scene_targets_share_one_blend_and_write_mask() {
    let states: Vec<wgpu::ColorTargetState> =
        SceneTargets::ColorAndAttachments.states(wgpu::TextureFormat::Rgba16Float).into_iter().flatten().collect();
    assert_eq!(states.len(), 1 + SCENE_ATTACHMENT_COUNT);
    assert!(states.iter().all(|state| state.blend == states[0].blend && state.write_mask == states[0].write_mask));
}

/// The deferred models compose into valid G-buffer and lighting modules, and their lighting passes read only the frame
/// and shadow groups the pass binds.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn deferred_models_compose_gbuffer_and_lighting_passes() {
    let common = scene_common_for(false);
    for (name, source) in builtin_models().into_iter().filter(|(name, _)| *name != "unlit") {
        for (pass, composed) in [
            ("g-buffer", gbuffer_module_source(&common, source)),
            ("lighting", lighting_module_source(&common, source)),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(&composed)
                .unwrap_or_else(|error| panic!("{name} {pass}: {}", error.emit_to_string(&composed)));
            let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|error| panic!("{name} {pass}: {error:?}"));
            if pass != "lighting" {
                continue;
            }
            let entry = module.entry_points.iter().position(|entry| entry.name == "lightingFragmentMain").unwrap();
            let usage = info.get_entry_point(entry);
            for (handle, variable) in module.global_variables.iter() {
                let group = variable.binding.as_ref().map_or(0, |binding| binding.group);
                assert!(group < 2 || usage[handle].is_empty(), "{name} lighting reads {:?}", variable.name);
            }
        }
    }
}
