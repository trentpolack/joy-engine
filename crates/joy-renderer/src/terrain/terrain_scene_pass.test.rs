// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/terrain/terrain_scene_pass.rs`.

use super::*;

#[test]
fn local_views_keep_screen_error_invariant_under_uniform_scale() {
    // A uniformly scaled model: scale 2, translation (8, 4, -6), eye (10, 8, -4).
    let model =
        Mat4::from_cols_array(&[2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 8.0, 4.0, -6.0, 1.0]);
    let inverse = invert_matrix(model.as_dmat4()).expect("invertible");
    let eye = inverse.transform_point3(DVec3::new(10.0, 8.0, -4.0));
    assert_eq!(eye, DVec3::new(1.0, 2.0, 1.0));
    let scale = 100.0 * stretch_bound(model.as_dmat4()) * stretch_bound(inverse);
    assert!((scale - 100.0).abs() < 1e-12);
    assert!(invert_matrix(DMat4::ZERO).is_none());
}

/// The lit terrain module validates on every backend: its forward entry points translate to WebGL 2 GLSL without
/// shadow samplers or extensions, and the G-buffer entry points validate for deferred lighting.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn lit_terrain_validates_and_its_forward_passes_translate_for_webgl() {
    use wgpu::naga::back::glsl;

    use crate::pipelines::scene_common_for;
    use crate::terrain::{
        TERRAIN_SCENE_SOURCE,
        terrain_geometry_source,
    };

    for gl in [true, false] {
        let source = format!(
            "{}\n{GBUFFER_ENCODING}\n{}\n{TERRAIN_SCENE_SOURCE}",
            scene_common_for(gl),
            terrain_geometry_source(LIT_GROUP)
        );
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("terrain: {}", error.emit_to_string(&source)));
        let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|error| panic!("terrain: {error:?}"));
        if !gl {
            assert!(module.entry_points.iter().any(|entry| entry.name == SceneTargets::Color.gbuffer_entry()));
            continue;
        }
        let options = glsl::Options {
            version: glsl::Version::Embedded {
                version: 300,
                is_webgl: true,
            },
            writer_flags: glsl::WriterFlags::TEXTURE_SHADOW_LOD,
            ..Default::default()
        };
        let stages = [
            (wgpu::naga::ShaderStage::Vertex, "vertexMain"),
            (wgpu::naga::ShaderStage::Fragment, "fragmentMain"),
            (wgpu::naga::ShaderStage::Fragment, "fragmentTargets"),
        ];
        for (stage, entry_point) in stages {
            let pipeline = glsl::PipelineOptions {
                shader_stage: stage,
                entry_point: entry_point.into(),
                multiview: None,
            };
            let mut output = String::new();
            glsl::Writer::new(&mut output, &module, &info, &options, &pipeline, Default::default())
                .and_then(|mut writer| writer.write().map(|_| ()))
                .unwrap_or_else(|error| panic!("terrain {entry_point}: {error:?}"));
            let shadow_sampler = output.contains("sampler2DShadow") || output.contains("sampler2DArrayShadow");
            assert!(!shadow_sampler && !output.contains("#extension"), "terrain {entry_point}");
        }
    }
}
