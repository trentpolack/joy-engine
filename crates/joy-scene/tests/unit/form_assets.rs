// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/form_assets.rs`.

use super::*;
use serde_json::json;

#[test]
fn lit_assets_keep_vertex_colors_and_pbr_assets_bake_materials() {
    let source = "color(0.5, 0.5, 0.5)\nbox(0, 0, 0, 2, 2, 2)";
    let palette = json!([{"name": "Default", "baseColor": [1.0, 0.0, 0.0, 1.0], "metallic": 0.25, "roughness": 0.5,
        "emissive": [0.0, 0.0, 0.0], "alphaMode": "OPAQUE", "alphaCutoff": 0.5}]);

    let lit = compile_form_mesh(source, &json!({}), Some(&palette), Shading::Lit).unwrap();
    let lit_vertex = lit.data().vertices[0];
    assert_eq!(&lit_vertex.color[..3], &[0.5, 0.5, 0.5]);

    let pbr = compile_form_mesh(source, &json!({}), Some(&palette), Shading::Pbr).unwrap();
    let pbr_vertex = pbr.data().vertices[0];
    assert_eq!(&pbr_vertex.color[..3], &[0.5, 0.0, 0.0]);
    assert_eq!(pbr_vertex.surface[0], 0.25);
    assert_eq!(pbr_vertex.surface[1], 0.5);
    assert_eq!(lit.bounds(), pbr.bounds());
}

#[test]
fn primitives_are_white_unit_shapes() {
    let cube = primitive_box();
    assert!(cube.data().vertices.iter().all(|vertex| vertex.color == [1.0; 4]));
    assert_eq!(cube.bounds().max - cube.bounds().min, joy_core::math::Vec3::ONE);
    let sphere = primitive_sphere();
    assert!((sphere.bounds().max.y - 0.5).abs() < 1e-5);
}

/// A lossless 2x1 WebP: half-transparent red, then opaque blue.
const WEBP: &str = "data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBIAAAAvAQAAEA8Q87//D/wPBxnR/wA=";

#[test]
fn textured_pbr_materials_keep_uvs_and_share_decoded_maps() {
    let source =
        "material(Painted)\nuv(0.25, 0.75)\ntriangle(0,0,0, 1,0,0, 0,1,0)\nmaterial()\ntriangle(0,0,1, 1,0,1, 0,1,1)";
    let palette = json!([{"name": "Painted", "baseColor": [1.0, 1.0, 1.0, 1.0], "metallic": 0.0, "roughness": 0.5,
        "emissive": [0.0, 0.0, 0.0], "alphaMode": "MASK", "alphaCutoff": 0.5,
        "textures": {"baseColor": WEBP, "normal": null, "emissive": null, "metallic": null, "roughness": null,
            "ao": null}}]);
    let geometry = joy_form::compile(source, &json!({}), Some(&palette)).unwrap();
    let mut cache = FormTextureCache::new();
    let asset = form_mesh_asset_with_textures(&geometry, Shading::Pbr, &mut cache).unwrap();
    assert_eq!(cache.len(), 1);
    let textured: Vec<_> = asset.submeshes().iter().filter(|submesh| submesh.textures.is_some()).collect();
    assert_eq!(textured.len(), 1, "only the painted triangle samples the map");
    assert!(textured[0].masked, "masked materials cast alpha-tested shadows");
    let first = textured[0].indices.start as usize;
    assert_eq!(asset.data().vertices[asset.data().indices[first] as usize].uv, [0.25, 0.75]);

    let again = form_mesh_asset_with_textures(&geometry, Shading::Pbr, &mut cache).unwrap();
    let maps = |asset: &MeshAsset| asset.submeshes().iter().find_map(|submesh| submesh.textures.clone()).unwrap();
    assert!(maps(&asset).maps().base_color.as_ref().unwrap().ptr_eq(maps(&again).maps().base_color.as_ref().unwrap()));

    let broken = palette.to_string().replace("UklGRh4A", "iVBORw0K");
    let geometry = joy_form::compile(source, &json!({}), Some(&serde_json::from_str(&broken).unwrap())).unwrap();
    assert!(matches!(form_mesh_asset(&geometry, Shading::Pbr), Err(RenderError::InvalidTexture(_))));
    assert!(form_mesh_asset(&geometry, Shading::Lit).is_ok(), "Lit assets ignore materials and their maps");
}

#[test]
fn invalid_source_reports_a_located_diagnostic() {
    let error = compile_form_mesh("box(", &json!({}), None, Shading::Lit).unwrap_err();
    assert!(matches!(error, FormMeshError::Form(diagnostic) if diagnostic.line == 1));
}
