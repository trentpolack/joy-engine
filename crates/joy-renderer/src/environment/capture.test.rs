// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/environment/capture.rs`.

use super::*;

#[test]
fn half_floats_match_ieee_encoding() {
    assert_eq!(encode_positive_half(0.0), 0);
    assert_eq!(encode_positive_half(1.0), 0x3c00);
    assert_eq!(encode_positive_half(0.5), 0x3800);
    assert_eq!(encode_positive_half(65504.0), 0x7bff);
    assert_eq!(encode_positive_half(2_f32.powi(-24)), 1);
    // Rounding up the mantissa carries into the exponent.
    assert_eq!(encode_positive_half(1.9999), 0x4000);
}

/// Each specular level's roughness inverts the lookup's `r(2 - r)` mapping, from the mirror image to roughness one.
#[test]
fn specular_levels_invert_the_lookup_mapping() {
    let levels = 6;
    assert_eq!(level_roughness(0, levels), 0.0);
    assert_eq!(level_roughness(levels - 1, levels), 1.0);
    for level in 0..levels {
        let roughness = level_roughness(level, levels);
        let lookup = roughness * (2.0 - roughness) * (levels - 1) as f32;
        assert!((lookup - level as f32).abs() < 1.0e-4, "level {level}: {lookup}");
    }
    assert_eq!(level_roughness(0, 1), 0.0);
}

/// Every capture pass validates and the passes WebGL 2 runs translate to its GLSL without extensions.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn capture_shaders_validate_and_translate_for_webgl() {
    use wgpu::naga::back::glsl;
    let modules = [
        (format!("{COMMON_SOURCE}\n{CAPTURE_SOURCE}"), vec!["equirectMain", "downsampleMain"]),
        (format!("{COMMON_SOURCE}\n{PREFILTER_SOURCE}"), vec!["prefilterMain"]),
        (format!("{COMMON_SOURCE}\n{BRDF_SOURCE}"), vec!["brdfMain"]),
    ];
    for (source, entries) in modules {
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{entries:?}: {}", error.emit_to_string(&source)));
        let info = wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|error| panic!("{entries:?}: {error:?}"));
        let options = glsl::Options {
            version: glsl::Version::Embedded {
                version: 300,
                is_webgl: true,
            },
            ..Default::default()
        };
        for entry_point in entries {
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
}
