// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/pass.rs`.

use super::*;

#[test]
fn layouts_follow_wgsl_alignment() {
    let fields = [
        UniformField { name: "a", kind: UniformKind::F32, default: &[1.0] },
        UniformField { name: "b", kind: UniformKind::Vec3, default: &[0.0; 3] },
        UniformField { name: "c", kind: UniformKind::Vec2, default: &[0.0; 2] },
        UniformField { name: "d", kind: UniformKind::Mat4, default: &[0.0; 16] },
    ];
    assert_eq!(uniform_layout(&fields), UniformLayout { offsets: vec![0, 16, 32, 48], size: 112 });
}

#[test]
fn packing_layers_overrides_over_defaults() {
    let pass = find_pass("depthAwareBlur").unwrap();
    let step: UniformValues = vec![("direction", vec![0.0, 1.0])];
    let frame: UniformValues = vec![("radius", vec![2.0])];
    let bytes = pack_uniforms(pass, &[&step, &frame]);
    let floats: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|word| f32::from_le_bytes(*word)).collect();
    // direction (vec2), radius, depthSigma, spatialSigma.
    assert_eq!(&floats[..5], &[0.0, 1.0, 2.0, 0.01, 3.0]);
}

#[test]
fn every_pass_assembles_with_named_bindings_and_matching_uniform_blocks() {
    for pass in PASSES {
        let (source, bindings) = assemble(pass);
        assert!(!source.contains("@binding(auto)"), "{}", pass.name);
        let names: Vec<&str> = bindings.iter().map(|binding| binding.name.as_str()).collect();
        assert!(names.contains(&"sourceTexture") && names.contains(&TEXTURE_TRANSFORM), "{}: {names:?}", pass.name);
        assert_eq!(
            bindings.iter().any(|binding| binding.kind == BindingKind::Uniform && binding.name == pass.name),
            !pass.uniforms.is_empty(),
            "{}",
            pass.name
        );
        // Every sampler belongs to a texture named before its `Sampler` suffix (some textures are only loaded).
        for binding in bindings.iter().filter(|binding| binding.kind == BindingKind::Sampler) {
            let texture = binding.name.strip_suffix("Sampler").unwrap_or_default();
            assert!(names.contains(&texture), "{}: {}", pass.name, binding.name);
        }
        // The WGSL uniform struct lists members in the table's order.
        if let Some(declaration) = source.split(&format!("var<uniform> {}:", pass.name)).nth(1) {
            let type_name = declaration.split(';').next().unwrap().trim();
            let body = source.split(&format!("struct {type_name} {{")).nth(1).unwrap().split('}').next().unwrap();
            let members: Vec<&str> = body
                .split(',')
                .filter_map(|member| member.split(':').next())
                .map(|member| member.trim().rsplit(|c: char| c.is_whitespace()).next().unwrap_or_default())
                .filter(|member| !member.is_empty() && !member.starts_with("//"))
                .collect();
            let table: Vec<&str> = pass.uniforms.iter().map(|field| field.name).collect();
            assert_eq!(members, table, "{}", pass.name);
        }
        wgpu::naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| panic!("{}: {error}", pass.name));
    }
}
