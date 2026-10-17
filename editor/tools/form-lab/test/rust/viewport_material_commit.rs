// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The preview keeps its displayed geometry paired with the accepted mesh, ported from
//! `test/viewport-material-commit.test.js`: a newer build whose material map fails to decode leaves the previous mesh,
//! markers, and framing in place.

use form_lab::rendering::viewport::Viewport;
use joy_engine::form::{
    GeometryData,
    default_material,
};
use joy_engine::serde_json::json;

/// A one-triangle mesh whose material carries `source` as its base color map.
fn mesh(source: Option<&str>, alpha_mode: &str) -> GeometryData {
    let mut material = default_material();
    material["roughness"] = json!(0.5);
    material["alphaMode"] = json!(alpha_mode);
    material["textures"]["baseColor"] = json!(source);
    GeometryData {
        positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        normals: vec![0.0; 9],
        colors: vec![1.0; 9],
        alphas: vec![1.0; 3],
        uvs: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        triangles: vec![0, 1, 2],
        triangle_materials: vec![0],
        materials: vec![material],
        ..GeometryData::default()
    }
}

#[test]
fn form_lab_retains_matching_point_data_when_a_newer_image_load_fails_after_a_successful_mesh_commit() {
    let mut viewport = Viewport::new();
    viewport.set_data(&mesh(None, "OPAQUE"), true).unwrap();
    let accepted = mesh(None, "BLEND");
    viewport.set_data(&accepted, false).unwrap();
    let revision = viewport.display_revision();
    let broken = mesh(Some("data:image/png;base64,AAAA"), "OPAQUE");
    let error = viewport.set_data(&broken, false).unwrap_err();
    assert!(error.starts_with("Material preview unavailable"), "{error}");
    assert_eq!(viewport.data(), Some(&accepted));
    assert_eq!(viewport.display_revision(), revision);
    assert_eq!(viewport.scene().len(), 1, "the accepted mesh stays drawn");
}

#[test]
fn point_only_builds_are_accepted_without_a_mesh() {
    let mut viewport = Viewport::new();
    viewport.set_data(&mesh(None, "OPAQUE"), true).unwrap();
    let points = GeometryData {
        positions: vec![1.0, 2.0, 3.0],
        colors: vec![1.0; 3],
        alphas: vec![1.0],
        points: vec![0],
        point_materials: vec![0],
        materials: vec![default_material()],
        ..GeometryData::default()
    };
    viewport.set_data(&points, true).unwrap();
    assert!(viewport.scene().is_empty());
    assert_eq!(viewport.camera.target.to_array(), [1.0, 2.0, 3.0]);
}
