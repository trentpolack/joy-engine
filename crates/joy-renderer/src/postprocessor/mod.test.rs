// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/postprocessor/mod.rs`.

use super::*;

fn bloom(radius: f32, quality: BloomQuality) -> PostProcessSettings {
    PostProcessSettings {
        bloom_strength: 1.0,
        bloom_radius: radius,
        bloom_quality: quality,
        ..Default::default()
    }
}

#[test]
fn bloom_reach_sets_the_pyramid_depth_and_blends_the_coarsest_level_smoothly() {
    // 0.05 of 720 pixels is a 36-pixel reach: 2^5.17, so six half-resolution levels with a sixth of the last one.
    let pyramid = BloomPyramid::new(&bloom(0.05, BloomQuality::High), (1280, 720));
    assert_eq!(
        pyramid.shape,
        BloomShape {
            levels: 6,
            full_resolution: false
        }
    );
    assert!((pyramid.coarsest_weight - 0.17).abs() < 0.01, "{}", pyramid.coarsest_weight);
    assert_eq!(pyramid.shape.level_size((1280, 720), 0), (640, 360));
    assert_eq!(pyramid.shape.level_size((1280, 720), 5), (20, 11));

    // Ultra starts at full resolution, adding a level for the same reach.
    let ultra = BloomPyramid::new(&bloom(0.05, BloomQuality::Ultra), (1280, 720));
    assert_eq!(
        ultra.shape,
        BloomShape {
            levels: 7,
            full_resolution: true
        }
    );
    assert_eq!(ultra.shape.level_size((1280, 720), 0), (1280, 720));

    // Long reaches stop before levels shrink below a few pixels; no strength means no pyramid.
    let small = BloomPyramid::new(&bloom(0.5, BloomQuality::High), (64, 32));
    assert!(small.shape.level_size((64, 32), small.shape.levels - 1).1 >= MIN_BLOOM_LEVEL_SIZE);
    let off = PostProcessSettings {
        bloom_strength: 0.0,
        ..bloom(0.05, BloomQuality::High)
    };
    assert_eq!(BloomPyramid::new(&off, (1280, 720)), BloomPyramid::NONE);
}

#[test]
fn graph_shape_follows_only_sizes_and_pyramid_depth() {
    let base = bloom(0.05, BloomQuality::High);
    let key = |config: &PostProcessSettings| GraphKey::new(config, (640, 360), (640, 360));
    // Uniform-only changes, including radius within the same level count, keep the graph.
    let tuned = PostProcessSettings {
        bloom_strength: 2.5,
        bloom_threshold: 0.4,
        bloom_knee: 0.9,
        bloom_scatter: 0.1,
        bloom_radius: 0.06,
        exposure: 1.8,
        tone_mapper: ToneMapper::Filmic,
        vignette_strength: 0.5,
        ..base.clone()
    };
    assert_eq!(key(&base), key(&tuned));
    // Reach across a level, quality's full-resolution start, and bloom on or off reshape it.
    assert_ne!(
        key(&base),
        key(&PostProcessSettings {
            bloom_radius: 0.2,
            ..base.clone()
        })
    );
    assert_ne!(
        key(&base),
        key(&PostProcessSettings {
            bloom_quality: BloomQuality::Ultra,
            ..base.clone()
        })
    );
    assert_eq!(
        key(&PostProcessSettings {
            bloom_strength: 0.0,
            ..base.clone()
        })
        .bloom
        .levels,
        0
    );
    // Only a scene smaller than the output upscales.
    let scaled = GraphKey::new(&base, (320, 180), (640, 360));
    assert!(scaled.upscales() && !key(&base).upscales());
}

#[test]
fn target_dither_by_their_precision() {
    assert_eq!(dither_amplitude(wgpu::TextureFormat::Bgra8UnormSrgb), 1.0 / 255.0);
    assert_eq!(dither_amplitude(wgpu::TextureFormat::Rgba16Float), 0.0);
}

/// Every bloom stage and the resolve validate and translate to WebGL 2 GLSL without extensions.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bloom_and_resolve_translate_for_webgl() {
    use wgpu::naga::back::glsl;
    let resolve = resolve_source();
    let stages = [
        (BLOOM_SOURCE, "prefilterMain"),
        (BLOOM_SOURCE, "downsampleMain"),
        (BLOOM_SOURCE, "upsampleMain"),
        (resolve.as_str(), "fragmentMain"),
    ];
    for (source, entry_point) in stages {
        let module = wgpu::naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|error| panic!("{entry_point}: {}", error.emit_to_string(source)));
        let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|error| panic!("{entry_point}: {error:?}"));
        let options = glsl::Options {
            version: glsl::Version::Embedded {
                version: 300,
                is_webgl: true,
            },
            ..Default::default()
        };
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
    assert_eq!(std::mem::size_of::<ResolveUniforms>(), 96);
}
