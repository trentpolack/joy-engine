// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Mesh and material resources, palettes, typed materials, and primitive helpers.

use joy_form::{
    AlphaMode,
    DEFAULT_SPHERE_SEGMENTS,
    FormMaterial,
    GeometryData,
    compile,
    mesh_box,
    mesh_sphere,
};
use serde_json::{
    Value,
    json,
};

fn evaluate(source: &str) -> GeometryData {
    compile(source, &json!({}), None).expect("valid FORM resource script")
}

fn material(name: &str) -> Value {
    json!({"name": name, "baseColor": [1,1,1,1], "metallic": 0,
        "roughness": 1, "emissive": [0,0,0], "alphaMode": "OPAQUE",
        "alphaCutoff": 0.5})
}

#[test]
fn reusable_box_preserves_source_geometry_and_restores_instance_attributes() {
    let data = evaluate(
        "let m=meshBox(2,4,6) normal(0,1,0) uv(0.25,0.75) instance(m,vector(10,0,0)) instance(m) point(0,0,0)",
    );
    assert_eq!(&data.positions[..3], &[9.0, -2.0, -3.0]);
    assert_eq!(&data.positions[24..27], &[-1.0, -2.0, -3.0]);
    assert_eq!(&data.triangles[..6], &[0, 2, 1, 0, 3, 2]);
    assert_eq!(&data.triangles[36..42], &[8, 10, 9, 8, 11, 10]);
    assert_eq!(&data.normals[48..], &[0.0, 1.0, 0.0]);
    assert_eq!(&data.uvs[32..], &[0.25, 0.75]);
    assert_eq!(&data.colors[..3], &[0.929, 0.424, 0.224]);
    assert_eq!(data.alphas, vec![1.0; 17]);
}

#[test]
fn sphere_emits_smooth_normals_restores_normal_and_leaves_final_uv() {
    let data = evaluate("normal(1,0,0) sphere(0,0,0,2,4) point(5,6,7)");
    assert_eq!(data.positions.len() / 3, 16);
    assert_eq!(data.triangles.len() / 3, 8);
    assert_eq!(&data.positions[..3], &[0.0, 2.0, 0.0]);
    assert_eq!(&data.normals[..3], &[0.0, 1.0, 0.0]);
    assert_eq!(&data.normals[45..], &[1.0, 0.0, 0.0]);
    assert_eq!(&data.uvs[30..], &[1.0, 1.0]);
}

#[test]
fn reflected_mesh_transforms_reverse_winding_and_use_inverse_scale_normals() {
    let source = "param shape=meshBox(1,1,1) instance(transformMesh(shape,vector(4,5,6),vector(0,0,0),vector(-2,3,4)))";
    let mesh = json!({"kind":"mesh","name":"Triangle", "positions":[0,0,0,1,0,0,0,1,0],
        "normals":[1,1,0,1,1,0,1,1,0],"uvs":[0,0,1,0,0,1],"indices":[0,1,2]});
    let data = compile(source, &json!({"shape":mesh.clone()}), None).unwrap();
    assert_eq!(data.positions, vec![4.0, 5.0, 6.0, 2.0, 5.0, 6.0, 4.0, 8.0, 6.0]);
    assert_eq!(data.triangles, vec![0, 2, 1]);
    assert!((data.normals[0] + 3.0 / 13_f64.sqrt()).abs() < 1e-12);
    assert!((data.normals[1] - 2.0 / 13_f64.sqrt()).abs() < 1e-12);
    assert_eq!(data.parameters[0]["value"], mesh);
    assert_eq!(data.parameters[0]["defaultValue"]["name"], "Box");
}

#[test]
fn bound_material_identity_is_reused_without_conflating_equal_values() {
    let data = evaluate(
        "let m=materialPbr(rgba(0.2,0.3,0.4,0.5),0.1,0.7) material(m) box(0,0,0,1,1,1) material(m) box(2,0,0,1,1,1) material(materialPbr(rgba(0.2,0.3,0.4,0.5),0.1,0.7)) box(4,0,0,1,1,1)",
    );
    assert_eq!(data.materials.len(), 3);
    assert_eq!(data.materials[1]["alphaMode"], "BLEND");
    assert_eq!(&data.triangle_materials[..24], &[1; 24]);
    assert_eq!(&data.triangle_materials[24..], &[2; 12]);
}

#[test]
fn palette_default_order_and_generated_names_preserve_authored_materials() {
    let palette = json!([material("ScriptMaterial_2"), material("Default")]);
    let data =
        compile("material(materialPbr(rgba(1,1,1,1),0,1)) box(0,0,0,1,1,1)", &json!({}), Some(&palette)).unwrap();
    assert_eq!(data.materials[0]["name"], "Default");
    assert_eq!(data.materials[1]["name"], "ScriptMaterial_2");
    assert_eq!(data.materials[2]["name"], "ScriptMaterial_3");
    assert_eq!(data.materials[1]["textures"]["normal"], Value::Null);
}

#[test]
fn borrowed_mesh_parameters_reject_incomplete_and_out_of_bounds_geometry() {
    let source = "param shape=meshBox(1,1,1) instance(shape)";
    let mut mesh = json!({"kind":"mesh","name":"Triangle","positions":[0,0,0,1,0,0,0,1,0],
        "normals":[0,0,0,0,0,0,0,0,0],"uvs":[0,0,1,0,0,1],"indices":[0,1,3]});
    let error = compile(source, &json!({"shape":mesh.clone()}), None).unwrap_err();
    assert!(error.message.contains("valid vertex indices"), "{error}");
    mesh["indices"] = json!([0, 1, 2]);
    mesh["uvs"] = json!([0, 0]);
    let error = compile(source, &json!({"shape":mesh}), None).unwrap_err();
    assert!(error.message.contains("match the vertex count"), "{error}");
    let error =
        compile("instance(meshBox(1,1,1),vector(0,0,0),vector(0,0,0),vector(1,0,1))", &json!({}), None).unwrap_err();
    assert!(error.message.contains("Mesh scale must be nonzero"), "{error}");
}

#[test]
fn material_validation_rejects_unknown_fields_duplicate_names_and_bad_textures() {
    let mut entry = material("Custom");
    entry["extra"] = json!(true);
    let error = compile("", &json!({}), Some(&json!([entry]))).unwrap_err();
    assert!(error.message.contains("unsupported field 'extra'"), "{error}");
    let error = compile("", &json!({}), Some(&json!([material("Custom"), material("Custom")]))).unwrap_err();
    assert!(error.message.contains("'Custom' is duplicated"), "{error}");
    for source in ["https://example.test/image.png", "data:image/png;base64,a===", "data:image/png;base64,"] {
        let mut entry = material("Custom");
        entry["textures"] = json!({"baseColor":source});
        assert!(compile("", &json!({}), Some(&json!([entry]))).is_err());
    }
}

#[test]
fn material_texture_allocation_limits_apply_to_individual_and_combined_sources() {
    let mut entry = material("Large");
    entry["textures"] = json!({"baseColor":format!("data:image/png;base64,{}", "A".repeat(5_333_336))});
    let error = compile("", &json!({}), Some(&json!([entry]))).unwrap_err();
    assert_eq!(error.message, "Textures must be 4 MB or smaller.");
    let texture = format!("data:image/png;base64,{}", "A".repeat(4_000_000));
    let palette: Vec<_> = (0..11)
        .map(|index| {
            let mut entry = material(&format!("Material_{index}"));
            entry["textures"] = json!({"baseColor":texture});
            entry
        })
        .collect();
    let error = compile("", &json!({}), Some(&json!(palette))).unwrap_err();
    assert_eq!(error.message, "Combined material textures must be 32 MB or smaller.");
}

#[test]
fn vertex_metadata_snapshots_preserve_null_gaps_and_clear_state() {
    let data = evaluate("point(0,0,0) userData({name: \"owned\", values: [1,2]}) point(1,0,0) userData() point(2,0,0)");
    assert_eq!(data.user_data, Some(vec![Value::Null, json!({"name":"owned","values":[1.0,2.0]}), Value::Null]));
}

#[test]
fn material_palette_converts_to_typed_records_with_document_spelling_and_defaults() {
    let mut paint = material("Paint");
    paint["baseColor"] = json!([0.3, 0.4, 0.5, 0.6]);
    paint["alphaMode"] = json!("MASK");
    paint["alphaCutoff"] = json!(0.25);
    paint["emissive"] = json!([4.0, 1.5, 0.0]);
    paint["textures"] = json!({"normal": "data:image/png;base64,iVBORw0KGgo="});
    let data = compile(
        "material(Paint) point(0,0,0) material(materialPbr(rgba(0.2,0.4,0.6,0.5),0.3,0.7)) point(1,0,0)",
        &json!({}),
        Some(&json!([paint])),
    )
    .unwrap();
    let materials = data.form_materials();
    assert_eq!(materials.len(), data.materials.len());
    assert_eq!(materials[0], FormMaterial::default());
    assert_eq!(materials[1].name, "Paint");
    assert_eq!(materials[1].base_color, [0.3, 0.4, 0.5, 0.6]);
    // HDR emission survives the palette and the typed view instead of falling back to black.
    assert_eq!(materials[1].emissive, [4.0, 1.5, 0.0]);
    assert_eq!((materials[1].alpha_mode, materials[1].alpha_cutoff), (AlphaMode::Mask, 0.25));
    assert_eq!(materials[1].textures.normal.as_deref(), Some("data:image/png;base64,iVBORw0KGgo="));
    assert_eq!(materials[1].textures.base_color, None);
    assert_eq!(materials[2].name, "ScriptMaterial_2");
    assert_eq!((materials[2].alpha_mode, materials[2].metallic, materials[2].roughness), (AlphaMode::Blend, 0.3, 0.7));
    assert_eq!(data.point_materials, vec![1, 2]);

    // Serialized typed records are the same FORM palette records, so they round-trip as palettes.
    for (typed, record) in materials.iter().zip(&data.materials) {
        assert_eq!(&serde_json::to_value(typed).unwrap(), record);
    }

    // Hand-edited records keep valid fields and fall back to `FormMaterial::default()` for the rest.
    let edited = FormMaterial::from_json(
        &json!({"name": "Edited", "metallic": 0.5, "roughness": 3, "alphaMode": "opaque", "emissive": [1, 0]}),
    );
    assert_eq!(
        edited,
        FormMaterial {
            name: "Edited".into(),
            metallic: 0.5,
            ..FormMaterial::default()
        }
    );
}

#[test]
fn primitive_helpers_match_the_recorded_mesh_box_and_mesh_sphere() {
    // Recorded expectations for `mesh_box(2, 3, 4)`, which the script `box(0,0,0,2,3,4)` also produces.
    let cube = mesh_box(2.0, 3.0, 4.0).unwrap();
    assert_eq!(
        cube.positions,
        vec![
            -1.0, -1.5, -2.0, 1.0, -1.5, -2.0, 1.0, 1.5, -2.0, -1.0, 1.5, -2.0, -1.0, -1.5, 2.0, 1.0, -1.5, 2.0, 1.0,
            1.5, 2.0, -1.0, 1.5, 2.0
        ]
    );
    assert_eq!(
        cube.triangles,
        vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5
        ]
    );
    assert_eq!((cube.normals.as_slice(), cube.uvs.as_slice()), (&[0.0; 24][..], &[0.0; 16][..]));
    assert_eq!((&cube.colors[..3], cube.alphas.as_slice()), (&[0.929, 0.424, 0.224][..], &[1.0; 8][..]));
    assert_eq!(
        (cube.triangle_materials.as_slice(), cube.form_materials()),
        (&[0; 12][..], vec![FormMaterial::default()])
    );
    assert_eq!(cube, evaluate("box(0,0,0,2,3,4)"));

    // Recorded expectations for `mesh_sphere(1.5, 6)`: 4 rows of 7 seam-duplicated vertices, pole rows emit one triangle
    // per column.
    let sphere = mesh_sphere(1.5, 6).unwrap();
    assert_eq!((sphere.positions.len() / 3, sphere.triangles.len() / 3), (28, 24));
    assert_eq!(&sphere.positions[..3], &[0.0, 1.5, 0.0]);
    assert_eq!(&sphere.triangles[..12], &[1, 8, 7, 2, 9, 8, 3, 10, 9, 4, 11, 10]);
    assert_close(
        &sphere.positions[24..30],
        &[0.6495190528383291, 0.7500000000000002, 1.1249999999999998, -0.6495190528383287, 0.7500000000000002, 1.125],
    );
    assert_close(&sphere.normals[24..27], &[0.4330127018922194, 0.5000000000000001, 0.7499999999999999]);
    assert_close(&sphere.uvs[16..18], &[0.16666666666666666, 0.3333333333333333]);
    assert_close(&sphere.normals[81..], &[1.2246467991473532e-16, -1.0, -2.999519565323715e-32]);
    assert_eq!(&sphere.uvs[54..], &[1.0, 1.0]);

    // Recorded expectations for a unit sphere at the default 20 segments.
    let sphere = mesh_sphere(1.0, DEFAULT_SPHERE_SEGMENTS).unwrap();
    assert_eq!((sphere.positions.len() / 3, sphere.triangles.len() / 3), (231, 360));
    assert_close(&sphere.normals[75..78], &[0.09549150281252629, 0.9510565162951535, 0.2938926261462365]);
    assert_eq!(&sphere.triangles[1074..], &[207, 208, 228, 208, 209, 229]);

    for (result, message) in [
        (mesh_box(0.0, 1.0, 1.0), "Box sizes must be positive."),
        (mesh_sphere(1.0, 2), "Expected an integer between 3 and 96."),
        (mesh_sphere(-1.0, 8), "Sphere radius must be positive."),
        (mesh_sphere(1.0, 97), "Expected an integer between 3 and 96."),
    ] {
        assert_eq!(result.unwrap_err().message, message);
    }
}

// Trigonometric values allow 1e-12 for last-bit differences in older recordings; arithmetic results are exact.
fn assert_close(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual - expected).abs() <= 1e-12, "expected {expected}, actual {actual}");
    }
}
