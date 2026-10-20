// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Materials through the language, documents, and point markers.

use form_lab::document::read_document;
use form_lab::rendering::batches::{
    MarkerCamera,
    point_batch,
};
use joy_engine::rendering::OrbitCamera;
use joy_engine::serde_json::{
    Value as Json,
    json,
};

use crate::support::compile;

fn steel() -> Json {
    json!({"name": "Steel", "baseColor": [0.7, 0.8, 0.9, 0.5], "metallic": 1, "roughness": 0.2,
        "emissive": [0.1, 0, 0], "alphaMode": "BLEND", "alphaCutoff": 0.5})
}

#[test]
fn named_material_selection_assigns_triangles_and_points_without_changing_vertex_colors() {
    let data = compile(
        "material(Steel); color(1,1,1); box(0,0,0,1,1,1); point(0,2,0); material(); point(1,2,0)",
        json!({}),
        Some(json!([steel()])),
    );
    assert_eq!(data.materials[1]["name"], "Steel");
    assert_eq!(data.triangle_materials, vec![1; 12]);
    assert_eq!(data.point_materials, [1, 0]);
    assert_eq!(data.colors[0], 1.0);
}

#[test]
fn material_selection_errors_retain_source_locations_and_do_not_coerce_expressions() {
    for source in ["material(Missing)", "material(1)", "material(Steel, Steel)"] {
        let error = joy_engine::form::compile(source, &json!({}), Some(&json!([steel()]))).unwrap_err();
        assert!(error.line == 1 && error.message.to_lowercase().contains("material"), "{error}");
    }
}

#[test]
fn document_materials_survive_saves_and_malformed_records_are_rejected() {
    let document = json!({"version": 1, "name": "Materials", "source": "material(Steel)", "overrides": {},
        "materials": [steel()]});
    let materials = read_document(&document.to_string()).unwrap().materials.unwrap();
    assert_eq!(materials.last().unwrap()["name"], "Steel");
    let mut invalid = document.clone();
    invalid["materials"][0]["metallic"] = json!(2);
    assert!(read_document(&invalid.to_string()).is_err());
}

#[test]
fn point_markers_preserve_material_tint_emission_and_alpha_modes() {
    let source = "material(Steel); color(1,1,1,0.5); point(0,0,0)";
    let camera = OrbitCamera::default();
    let marker_camera = MarkerCamera { basis: camera.basis(), distance: camera.distance };
    let preview = |material: Json| {
        point_batch(&compile(source, json!({}), Some(json!([material]))), false, &marker_camera, 2.0, 800.0)
    };
    let result = preview(steel());
    assert_eq!(result.len(), 6);
    assert_eq!(result[0].color[3], 0.25);
    let mut unlit = steel();
    unlit["emissive"] = json!([0, 0, 0]);
    assert_ne!(result, preview(unlit));
    let mut opaque = steel();
    opaque["alphaMode"] = json!("OPAQUE");
    assert_eq!(preview(opaque)[0].color[3], 1.0);
    let mut masked = steel();
    masked["alphaMode"] = json!("MASK");
    assert_eq!(preview(masked)[0].color[3], 0.0);
    // Red is tint × base color + emission: 1 × 0.7 + 0.1.
    assert!((result[0].color[0] - 0.8).abs() < 1e-6);
}
