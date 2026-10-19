// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/postprocessor/mod.rs`.

use super::*;
use crate::post_process::BloomQuality;

#[test]
fn pyramid_levels_round_like_luma() {
    // 1366 × 768: a quarter of 1366 is 341.5, which rounds half up to 342, as luma.gl rounds level sizes.
    let sizes: Vec<(u32, u32)> =
        BLOOM_LEVEL_SCALES.iter().map(|scale| (level_size(1366, *scale), level_size(768, *scale))).collect();
    assert_eq!(sizes, [(683, 384), (342, 192), (171, 96), (85, 48), (43, 24)]);
    assert_eq!(level_size(1, 0.03125), 1);
}

#[test]
fn texture_scale_corrects_aspect_like_luma() {
    assert_eq!(texture_scale((1920, 1080), (960, 540)), [1.0, 1.0]);
    assert_eq!(texture_scale((100, 100), (200, 100)), [1.0, 2.0]);
    assert_eq!(texture_scale((200, 100), (100, 100)), [2.0, 1.0]);
}

#[test]
fn anamorphic_stretch_keeps_each_axis_within_the_blur_limit() {
    let config = |radius: f32, anamorphic: f32| PostProcessSettings {
        bloom_radius: radius,
        bloom_anamorphic: anamorphic,
        ..PostProcessSettings::default()
    };
    // 0.025 of 720 pixels is eight texels; a ratio of 0.5 stretches the horizontal axis to twelve.
    let (horizontal, vertical) = bloom_blur_radii(&config(0.025, 0.5), 720.0);
    assert!((horizontal - 12.0).abs() < 1e-4 && (vertical - 8.0).abs() < 1e-4, "{horizontal} {vertical}");
    // At the 24-texel cap the stretched axis stays at 24 and the other shrinks to 16.
    let (horizontal, vertical) = bloom_blur_radii(&config(0.25, 0.5), 720.0);
    assert!((horizontal - 24.0).abs() < 1e-4 && (vertical - 16.0).abs() < 1e-4, "{horizontal} {vertical}");
    assert_eq!(bloom_blur_radii(&config(0.0, 1.0), 720.0), (0.0, 0.0));
}

#[test]
fn graph_shape_follows_only_pipeline_key_values() {
    let base = PostProcessSettings { bloom_strength: 1.0, ..PostProcessSettings::default() };
    let key = |config: &PostProcessSettings| GraphKey::new(config, (640, 360), (640, 360), false);
    // Uniform-only changes keep the graph.
    let tuned = PostProcessSettings {
        bloom_strength: 2.5,
        bloom_threshold: 0.4,
        bloom_knee: 0.9,
        bloom_scatter: 0.1,
        bloom_radius: 0.08,
        bloom_anamorphic: 0.7,
        exposure: 1.8,
        ..base.clone()
    };
    assert_eq!(key(&base), key(&tuned));
    let streaks = PostProcessSettings { bloom_lens_flare: 0.5, ..base.clone() };
    assert_eq!(key(&streaks), key(&PostProcessSettings { bloom_lens_flare: 2.0, ..streaks.clone() }));
    // Bloom on/off, quality, and streaks on/off reshape it.
    assert_ne!(key(&base), key(&streaks));
    assert_ne!(key(&base), key(&PostProcessSettings { bloom_quality: BloomQuality::Low, ..base.clone() }));
    assert_eq!(key(&PostProcessSettings { bloom_strength: 0.0, ..streaks.clone() }).bloom_levels, 0);
    assert!(!key(&PostProcessSettings { bloom_strength: 0.0, ..streaks }).lens_flare);
    // A later blur moves the composite ahead of it.
    assert!(!key(&base).composes_bloom_early());
    assert!(key(&PostProcessSettings { vignette_blur: 0.5, ..base.clone() }).composes_bloom_early());
    assert!(key(&PostProcessSettings { blur_radius: 0.01, ..base.clone() }).composes_bloom_early());
    // A reduced render scale and an extended display reshape it; only a scene smaller than the output upscales.
    let scaled = GraphKey::new(&base, (320, 180), (640, 360), false);
    assert_ne!(key(&base), scaled);
    assert!(scaled.upscales() && !key(&base).upscales());
    assert_ne!(key(&base), GraphKey::new(&base, (640, 360), (640, 360), true));
}

/// Every bloom stage and the resolve validate and translate to WebGL 2 GLSL without extensions.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bloom_and_resolve_translate_for_webgl() {
    use wgpu::naga::back::glsl;
    let stages = [
        (BLOOM_SOURCE, "extractMain"),
        (BLOOM_SOURCE, "downsampleMain"),
        (BLOOM_SOURCE, "blurMain"),
        (BLOOM_SOURCE, "upsampleMain"),
        (BLOOM_SOURCE, "lensMain"),
        (BLOOM_SOURCE, "compositeMain"),
        (RESOLVE_SOURCE, "fragmentMain"),
    ];
    for (source, entry_point) in stages {
        let module =
            wgpu::naga::front::wgsl::parse_str(source).unwrap_or_else(|error| panic!("{entry_point}: {error}"));
        let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|error| panic!("{entry_point}: {error:?}"));
        let options =
            glsl::Options { version: glsl::Version::Embedded { version: 300, is_webgl: true }, ..Default::default() };
        let pipeline = glsl::PipelineOptions {
            shader_stage: wgpu::naga::ShaderStage::Fragment,
            entry_point: entry_point.into(),
            multiview: None,
        };
        let mut output = String::new();
        glsl::Writer::new(&mut output, &module, &info, &options, &pipeline, Default::default())
            .and_then(|mut writer| writer.write().map(|_| ()))
            .unwrap_or_else(|error| panic!("{entry_point}: {error:?}"));
        assert!(!output.contains("#extension"), "{entry_point} needs a GLSL extension");
    }
}

#[test]
fn bloom_uniform_block_matches_the_shader_layout() {
    assert_eq!(std::mem::size_of::<BloomUniforms>(), 48);
    let source = BLOOM_SOURCE;
    let members = source.split("struct Bloom {").nth(1).unwrap().split("};").next().unwrap();
    let names: Vec<&str> = members
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .filter_map(|line| line.split(':').next())
        .collect();
    assert_eq!(
        names,
        [
            "textureScale",
            "delta",
            "threshold",
            "softKnee",
            "exposure",
            "radius",
            "scatter",
            "intensity",
            "starburstIntensity",
            "starburstLength"
        ]
    );
}
