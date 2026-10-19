// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GLB export, plus byte-for-byte comparisons with recorded reference GLBs for the same inputs
//! (`tests/fixtures/*.glb`).

use form_lab::export_glb::export_glb;
use joy_engine::form::default_material;
use joy_engine::serde_json::json;

use crate::support::{
    compile,
    fixture,
    glass,
    parse_glb,
    read_accessor,
    sample_geometry,
};

#[test]
fn glb_contains_aligned_json_and_binary_chunks_with_declared_total_length() {
    let glb = export_glb(&sample_geometry()).unwrap();
    let word = |offset: usize| u32::from_le_bytes(glb[offset..offset + 4].try_into().unwrap());
    assert_eq!(word(0), 0x4654_6c67);
    assert_eq!(word(4), 2);
    assert_eq!(word(8) as usize, glb.len());
    let json_length = word(12) as usize;
    assert_eq!(json_length % 4, 0);
    assert_eq!(word(16), 0x4e4f_534a);
    let binary_header = 20 + json_length;
    assert_eq!(word(binary_header + 4), 0x004e_4942);
    assert_eq!(word(binary_header) % 4, 0);
    assert_eq!(binary_header + 8 + word(binary_header) as usize, glb.len());
}

#[test]
fn glb_preserves_mixed_triangle_and_point_assignments_flat_normals_and_rgba() {
    let parsed = parse_glb(&export_glb(&sample_geometry()).unwrap());
    let primitives = parsed.json["meshes"][0]["primitives"].as_array().unwrap().clone();
    assert_eq!(primitives.len(), 2);
    let triangle = primitives.iter().find(|primitive| primitive["mode"] == 4).unwrap();
    let point = primitives.iter().find(|primitive| primitive["mode"] == 0).unwrap();
    assert_eq!(triangle["material"], 1);
    assert_eq!(point["material"], 0);
    let position = &triangle["attributes"]["POSITION"];
    assert_eq!(parsed.json["accessors"][position.as_u64().unwrap() as usize]["count"], 3);
    assert_eq!(
        read_accessor(&parsed, &triangle["attributes"]["NORMAL"]),
        [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(
        read_accessor(&parsed, &triangle["attributes"]["COLOR_0"]),
        [1.0, 0.0, 0.0, 0.25, 0.0, 1.0, 0.0, 0.5, 0.0, 0.0, 1.0, 0.75]
    );
    assert_eq!(read_accessor(&parsed, &point["attributes"]["POSITION"]), [2.0, 3.0, 4.0]);
    let accessor = &parsed.json["accessors"][position.as_u64().unwrap() as usize];
    assert_eq!(accessor["min"], json!([0, 0, 0]));
    assert_eq!(accessor["max"], json!([1, 1, 0]));
}

#[test]
fn glb_duplicates_shared_hard_edge_vertices_and_keeps_authored_normals_uvs_colors_and_materials_aligned() {
    let parsed = parse_glb(&export_glb(&compile("box(0,0,0,2,2,2)", json!({}), None)).unwrap());
    let primitive = &parsed.json["meshes"][0]["primitives"][0];
    let positions = read_accessor(&parsed, &primitive["attributes"]["POSITION"]);
    let normals = read_accessor(&parsed, &primitive["attributes"]["NORMAL"]);
    let mut corner_normals = Vec::new();
    for index in (0..positions.len()).step_by(3) {
        if positions[index..index + 3].iter().all(|value| *value == -1.0) {
            corner_normals.push(normals[index..index + 3].iter().map(|value| value + 0.0).collect::<Vec<f32>>());
        }
    }
    let distinct: std::collections::HashSet<String> =
        corner_normals.iter().map(|normal| format!("{normal:?}")).collect();
    assert_eq!(distinct.len(), 3);
    assert!(corner_normals.iter().all(|normal| normal.iter().filter(|component| **component != 0.0).count() == 1));

    let palette = json!([default_material(), glass()]);
    let data = compile(
        "material(Glass) uv(0.25,0.75) color(1,0,0,0.5) normal(0,3,4) triangle(0,0,0,1,0,0,0,1,0) material() normal() \
         uv(0.5,1) triangle(0,0,0,1,0,0,0,1,0)",
        json!({}),
        Some(palette),
    );
    let parsed = parse_glb(&export_glb(&data).unwrap());
    let primitives = parsed.json["meshes"][0]["primitives"].as_array().unwrap().clone();
    let authored = primitives.iter().find(|primitive| primitive["material"] == 1).unwrap();
    let flat = primitives.iter().find(|primitive| primitive["material"] == 0).unwrap();
    assert_eq!(read_accessor(&parsed, &authored["attributes"]["NORMAL"]), [0.0, 0.6, 0.8].repeat(3));
    assert_eq!(read_accessor(&parsed, &authored["attributes"]["TEXCOORD_0"]), [0.25, 0.75].repeat(3));
    assert_eq!(read_accessor(&parsed, &authored["attributes"]["COLOR_0"]), [1.0, 0.0, 0.0, 0.5].repeat(3));
    assert_eq!(read_accessor(&parsed, &flat["attributes"]["NORMAL"]), [0.0, 0.0, 1.0].repeat(3));
    assert_eq!(read_accessor(&parsed, &flat["attributes"]["TEXCOORD_0"]), [0.5, 1.0].repeat(3));
}

#[test]
fn glb_materials_preserve_pbr_alpha_and_two_sided_properties() {
    let parsed = parse_glb(&export_glb(&sample_geometry()).unwrap());
    assert!(parsed.json["materials"][0].get("alphaCutoff").is_none());
    assert_eq!(
        parsed.json["materials"][1],
        json!({
            "name": "Glass",
            "pbrMetallicRoughness": {"baseColorFactor": [0.2, 0.3, 0.4, 0.6], "metallicFactor": 0.7, "roughnessFactor": 0.1},
            "emissiveFactor": [0.05, 0.1, 0.15],
            "alphaMode": "MASK",
            "alphaCutoff": 0.4,
            "doubleSided": true
        })
    );
    assert!(parsed.json.get("extensionsUsed").is_none(), "no extension is declared without a use");

    // HDR emission exports as a normalized factor plus KHR_materials_emissive_strength, which glTF readers without
    // the extension ignore gracefully.
    let mut glowing = sample_geometry();
    glowing.materials[1]["emissive"] = json!([4.0, 2.0, 0.0]);
    let parsed = parse_glb(&export_glb(&glowing).unwrap());
    assert_eq!(parsed.json["materials"][1]["emissiveFactor"], json!([1, 0.5, 0]));
    assert_eq!(
        parsed.json["materials"][1]["extensions"],
        json!({"KHR_materials_emissive_strength": {"emissiveStrength": 4}})
    );
    assert_eq!(parsed.json["extensionsUsed"], json!(["KHR_materials_emissive_strength"]));
    assert!(parsed.json.get("extensionsRequired").is_none());
}

#[test]
fn glb_bounds_describe_serialized_float32_values_exactly() {
    let mut data = sample_geometry();
    data.positions[3] = 1.0 / 3.0;
    let parsed = parse_glb(&export_glb(&data).unwrap());
    let primitives = parsed.json["meshes"][0]["primitives"].as_array().unwrap();
    let triangle = primitives.iter().find(|primitive| primitive["mode"] == 4).unwrap();
    let accessor = &parsed.json["accessors"][triangle["attributes"]["POSITION"].as_u64().unwrap() as usize];
    assert_eq!(accessor["max"][0].as_f64().unwrap(), f64::from((1.0_f64 / 3.0) as f32));
}

#[test]
fn glb_rejects_material_reordering_float32_overflow_and_out_of_range_vertex_colors() {
    let mut missing_default = sample_geometry();
    missing_default.materials = vec![missing_default.materials[1].clone()];
    missing_default.triangle_materials = vec![0];
    assert!(export_glb(&missing_default).unwrap_err().0.contains("'Default' at index 0"));

    let mut overflowing = sample_geometry();
    overflowing.positions[0] = 1e40;
    assert!(export_glb(&overflowing).unwrap_err().0.contains("float32"));

    let mut invalid_color = sample_geometry();
    invalid_color.alphas[0] = 1.1;
    assert!(export_glb(&invalid_color).unwrap_err().0.contains("from 0 to 1"));

    let mut collapsed = sample_geometry();
    collapsed.positions[3] = 1e-50;
    assert!(export_glb(&collapsed).unwrap_err().0.contains("degenerate"));
}

#[test]
fn glb_rejects_empty_or_unsupported_geometry_instead_of_silently_dropping_it() {
    let mut empty = sample_geometry();
    empty.triangles.clear();
    empty.points.clear();
    assert!(export_glb(&empty).unwrap_err().0.contains("empty"));
    let mut unassigned = sample_geometry();
    unassigned.triangle_materials.clear();
    assert!(export_glb(&unassigned).unwrap_err().0.contains("assignment"));
    let mut out_of_range = sample_geometry();
    out_of_range.points = vec![99];
    assert!(export_glb(&out_of_range).unwrap_err().0.contains("index"));
}

#[test]
fn glb_declares_the_required_webp_extension_without_inventing_a_core_fallback() {
    let mut geometry = sample_geometry();
    let mut textured = default_material();
    for (key, value) in glass().as_object().unwrap() {
        textured[key] = value.clone();
    }
    textured["textures"]["baseColor"] = json!("data:image/webp;base64,UklGRg==");
    geometry.materials[1] = textured.clone();
    let parsed = parse_glb(&export_glb(&geometry).unwrap());
    assert_eq!(parsed.json["extensionsUsed"], json!(["EXT_texture_webp"]));
    assert_eq!(parsed.json["extensionsRequired"], json!(["EXT_texture_webp"]));
    assert_eq!(parsed.json["images"][0]["mimeType"], "image/webp");
    assert_eq!(parsed.json["textures"][0], json!({"extensions": {"EXT_texture_webp": {"source": 0}}}));
    textured["textures"]["baseColor"] = json!("data:image/png;base64,iVBORw==");
    geometry.materials[1] = textured;
    let png = parse_glb(&export_glb(&geometry).unwrap()).json;
    assert!(png.get("extensionsRequired").is_none());
    assert_eq!(png["textures"][0], json!({"source": 0}));
}

/// The recorded reference GLBs for the same inputs, byte for byte: JSON key order, number formatting, padding, and
/// binary layout.
#[test]
fn glb_bytes_match_the_recorded_export() {
    let palette = json!([default_material(), glass()]);
    let mut third = sample_geometry();
    third.positions[3] = 1.0 / 3.0;
    let authored = compile(
        "material(Glass) uv(0.25,0.75) color(1,0,0,0.5) normal(0,3,4) triangle(0,0,0,1,0,0,0,1,0) material() normal() \
         uv(0.5,1) triangle(0,0,0,1,0,0,0,1,0)",
        json!({}),
        Some(palette),
    );
    let mut webp = sample_geometry();
    let mut textured = default_material();
    for (key, value) in glass().as_object().unwrap() {
        textured[key] = value.clone();
    }
    textured["textures"]["baseColor"] = json!("data:image/webp;base64,UklGRg==");
    textured["textures"]["roughness"] = json!("data:image/png;base64,iVBORw==");
    textured["textures"]["ao"] = json!("data:image/png;base64,iVBORw==");
    webp.materials[1] = textured;
    for (name, data) in [
        ("sample.glb", sample_geometry()),
        ("sample-third.glb", third),
        ("box.glb", compile("box(0,0,0,2,2,2)", json!({}), None)),
        ("authored.glb", authored),
        ("webp.glb", webp),
    ] {
        let exported = export_glb(&data).unwrap();
        let reference = fixture(name);
        if exported != reference {
            let ours = parse_glb(&exported).json.to_string();
            let theirs = parse_glb(&reference).json.to_string();
            panic!("{name} differs from the reference export:\n{ours}\n{theirs}");
        }
    }
}

/// The Material studio build exports the same GLB as the reference exports (set `FORM_LAB_REFERENCE_EXPORTS`).
#[test]
fn material_studio_glb_matches_the_reference_exports() {
    let Ok(reference) = std::env::var("FORM_LAB_REFERENCE_EXPORTS") else {
        eprintln!("skipping: set FORM_LAB_REFERENCE_EXPORTS to a folder of reference material-studio exports");
        return;
    };
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/examples/material-studio.form"),
    )
    .unwrap();
    let exported = export_glb(&compile(&source, json!({}), None)).unwrap();
    let expected = std::fs::read(std::path::Path::new(&reference).join("material-studio.glb")).unwrap();
    assert_eq!(exported.len(), expected.len());
    let differing = exported.iter().zip(&expected).filter(|(left, right)| left != right).count();
    assert_eq!(parse_glb(&exported).json, parse_glb(&expected).json);
    // FORM's transcendental results may differ from the reference exporter's in the last bit, which can flip a float32
    // rounding.
    assert!(differing * 1000 < exported.len(), "{differing} of {} bytes differ", exported.len());
    eprintln!("material studio GLB: {differing} of {} bytes differ", exported.len());
}
