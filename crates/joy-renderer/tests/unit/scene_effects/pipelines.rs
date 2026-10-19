// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/pipelines.rs`.

use super::*;
use crate::scene_effects::pass::find_pass;

fn everything() -> SceneEffectsOptions {
    SceneEffectsOptions {
        depth_aware_blur: Some(Default::default()),
        gtao: Some(Default::default()),
        ssgi: Some(Default::default()),
        outlines: Some(Default::default()),
        taa: Some(Default::default()),
        motion_blur: Some(Default::default()),
        ssr: Some(Default::default()),
        height_fog: Some(Default::default()),
        clustered_lighting: Some(Default::default()),
        adaptive_exposure: Some(Default::default()),
        ..Default::default()
    }
}

#[test]
fn pipelines_route_only_to_known_passes_and_targets() {
    for options in [everything(), SceneEffectsOptions { ssao: Some(Default::default()), ..Default::default() }] {
        let groups = scene_effect_pipelines(&options, Rgba16Float);
        for pipeline in groups.lighting.iter().chain(&groups.temporal).chain(&groups.finishing) {
            let names: Vec<&str> = pipeline.targets.iter().map(|target| target.name).collect();
            for step in &pipeline.steps {
                assert!(find_pass(step.pass).is_some(), "{} uses unknown pass {}", pipeline.name, step.pass);
                for (_, source) in &step.inputs {
                    if let Target(name) = source {
                        assert!(names.contains(name), "{}: unknown input {name}", pipeline.name);
                    }
                }
                if let Some(output) = step.output {
                    assert!(names.contains(&output), "{}: unknown output {output}", pipeline.name);
                }
            }
        }
    }
}

#[test]
fn joy_overrides_apply_formats_and_samplers() {
    let groups = scene_effect_pipelines(&everything(), Rgba8Unorm);
    let all: Vec<&TargetSpec> =
        groups.lighting.iter().chain(&groups.temporal).flat_map(|pipeline| &pipeline.targets).collect();
    let find = |name: &str| all.iter().find(|target| target.name == name).unwrap();
    assert_eq!(find("gtaoHistoryDepth").format, wgpu::TextureFormat::Rgba32Float);
    assert_eq!(find("gtaoHistoryDepth").sampling, TargetSampling::Nearest);
    assert_eq!(find("fogHistory").format, Rgba8Unorm);
    assert_eq!(find("taaHistoryColor").format, Rgba8Unorm);
    assert_eq!(find("ssgiRaw").scale, 0.5);
    assert_eq!(find("clusteredVolumeDepthHistory").format, Rg16Float);
    assert_eq!(groups.temporal.len(), 1);
    assert_eq!(
        groups.finishing.iter().map(|pipeline| pipeline.name).collect::<Vec<_>>(),
        ["motionBlurShaderPassPipeline", "hdrAutoExposureShaderPassPipeline"]
    );
}
