// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/runtime.rs`.

use super::*;
use crate::scene_effects::config::{
    AdaptiveExposureOptions,
    MotionBlurOptions,
    TaaOptions,
    VolumetricOptions,
};

fn every_effect() -> SceneEffectsOptions {
    SceneEffectsOptions {
        taa: Some(TaaOptions::default()),
        motion_blur: Some(MotionBlurOptions::default()),
        adaptive_exposure: Some(AdaptiveExposureOptions::default()),
        volumetrics: Some(VolumetricOptions::default()),
    }
}

fn availability(report: &SceneEffectsCapabilities) -> Vec<(&'static str, bool, Option<&'static str>)> {
    report.effects.iter().map(|effect| (effect.name, effect.available, effect.reason)).collect()
}

#[test]
fn capabilities_fall_back_by_what_each_effect_needs() {
    // WebGPU and native backends run everything.
    let full = capabilities(
        &every_effect(),
        EffectSupport {
            compute: true,
            float_targets: true,
        },
    );
    assert!(full.available && full.effects.iter().all(|effect| effect.available && effect.reason.is_none()));

    // WebGL 2 with float targets runs everything but volumetric fog.
    let webgl = capabilities(
        &every_effect(),
        EffectSupport {
            compute: false,
            float_targets: true,
        },
    );
    assert!(webgl.available);
    assert_eq!(
        availability(&webgl),
        [
            (TAA_NAME, true, None),
            (MOTION_BLUR_NAME, true, None),
            (ADAPTIVE_EXPOSURE_NAME, true, None),
            (VOLUMETRICS_NAME, false, Some(COMPUTE_REQUIRED)),
        ]
    );

    // Without float targets nothing runs, and only requested effects report a reason.
    let options = SceneEffectsOptions {
        taa: Some(TaaOptions::default()),
        ..Default::default()
    };
    let limited = capabilities(
        &options,
        EffectSupport {
            compute: false,
            float_targets: false,
        },
    );
    assert!(!limited.available);
    assert_eq!(limited.effects[0].reason, Some(FLOAT_TARGETS_REQUIRED));
    assert!(limited.effects[1..].iter().all(|effect| !effect.requested && effect.reason.is_none()));
}

/// Every effect shader validates; the passes that run on WebGL 2 also translate to its GLSL without extensions.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn effect_shaders_validate_and_the_webgl_passes_translate() {
    use wgpu::naga::back::glsl;

    use crate::pipelines::FULLSCREEN_SOURCE;

    let fullscreen = |source: &str| format!("{FULLSCREEN_SOURCE}\n{source}");
    let scene_common = crate::pipelines::scene_common_for(false);
    let volumetric_common = include_str!("shaders/volumetric-common.wgsl");

    // (name, source, fragment entry points to translate for WebGL 2)
    let modules = [
        ("temporal", fullscreen(include_str!("shaders/temporal.wgsl")), vec!["fragmentMain"]),
        (
            "motion blur",
            fullscreen(include_str!("shaders/motion-blur.wgsl")),
            vec!["tileMaxMain", "neighborMaxMain", "gatherMain"],
        ),
        (
            "exposure fallback",
            fullscreen(include_str!("shaders/exposure-fallback.wgsl")),
            vec!["meterMain", "reduceMain", "adaptMain"],
        ),
        ("exposure histogram", include_str!("shaders/exposure.wgsl").to_owned(), vec![]),
        (
            "volumetric inject",
            format!("{scene_common}\n{volumetric_common}\n{}", include_str!("shaders/volumetric-inject.wgsl")),
            vec![],
        ),
        (
            "volumetric integrate",
            format!("{volumetric_common}\n{}", include_str!("shaders/volumetric-integrate.wgsl")),
            vec![],
        ),
        ("volumetric apply", fullscreen(include_str!("shaders/volumetric-apply.wgsl")), vec![]),
    ];
    for (name, source, webgl_entries) in modules {
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(&source)));
        let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let options = glsl::Options {
            version: glsl::Version::Embedded {
                version: 300,
                is_webgl: true,
            },
            ..Default::default()
        };
        for entry_point in webgl_entries {
            let pipeline = glsl::PipelineOptions {
                shader_stage: wgpu::naga::ShaderStage::Fragment,
                entry_point: entry_point.into(),
                multiview: None,
            };
            let mut output = String::new();
            glsl::Writer::new(&mut output, &module, &info, &options, &pipeline, Default::default())
                .and_then(|mut writer| writer.write().map(|_| ()))
                .unwrap_or_else(|error| panic!("{name} {entry_point}: {error:?}"));
            assert!(!output.contains("#extension"), "{name} {entry_point} needs a GLSL extension");
        }
    }
}
