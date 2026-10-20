// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/form_assets.rs`.

use super::*;
use joy_render_types::{
    AlphaMode,
    Shading,
    Surface,
};
use serde_json::json;

#[test]
fn flat_assets_share_one_white_material_and_layered_assets_keep_the_palette() {
    let source = "color(0.5, 0.5, 0.5)\nbox(0, 0, 0, 2, 2, 2)";
    let palette = json!([{"name": "Default", "baseColor": [1.0, 0.0, 0.0, 1.0], "metallic": 0.25, "roughness": 0.5,
        "emissive": [0.0, 0.0, 0.0], "alphaMode": "OPAQUE", "alphaCutoff": 0.5}]);

    // FORM's flat look ignores the palette: one slot, drawn white, so vertex colors show as authored.
    let lit = compile_form_mesh(source, &json!({}), Some(&palette), ShadingModel::Lit).unwrap();
    assert_eq!(lit.slot_count(), 1);
    assert!(
        matches!(&lit.materials()[0].material().shading, Shading::Lit(surface) if surface.base_color == joy_core::LinearColor::WHITE)
    );

    // Layered assets keep every palette factor as a surface layer.
    let layered = compile_form_mesh(source, &json!({}), Some(&palette), ShadingModel::Layered).unwrap();
    let Shading::Layered(Surface::Layer(layer)) = &layered.materials()[0].material().shading else {
        panic!("FORM palettes become single surface layers");
    };
    assert_eq!((layer.base_color.red, layer.base_color.green, layer.metallic, layer.roughness), (1.0, 0.0, 0.25, 0.5));
    assert_eq!(lit.bounds(), layered.bounds());
    assert_eq!(lit.triangle_count(), layered.triangle_count());
}

#[test]
fn primitives_are_white_unit_shapes() {
    let cube = primitive_box();
    assert!(!cube.has_vertex_alpha());
    assert_eq!(cube.bounds().max - cube.bounds().min, joy_core::math::Vec3::ONE);
    let sphere = primitive_sphere();
    assert!((sphere.bounds().max.y - 0.5).abs() < 1e-5);
}

/// A lossless 2x1 WebP: half-transparent red, then opaque blue.
const WEBP: &str = "data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBIAAAAvAQAAEA8Q87//D/wPBxnR/wA=";

#[test]
fn textured_layered_materials_share_decoded_maps_by_slot() {
    let source =
        "material(Painted)\nuv(0.25, 0.75)\ntriangle(0,0,0, 1,0,0, 0,1,0)\nmaterial()\ntriangle(0,0,1, 1,0,1, 0,1,1)";
    let palette = json!([{"name": "Painted", "baseColor": [1.0, 1.0, 1.0, 1.0], "metallic": 0.0, "roughness": 0.5,
        "emissive": [0.0, 0.0, 0.0], "alphaMode": "MASK", "alphaCutoff": 0.5,
        "textures": {"baseColor": WEBP, "normal": null, "emissive": null, "metallic": null, "roughness": null,
            "ao": null}}]);
    let geometry = joy_form::compile(source, &json!({}), Some(&palette)).unwrap();
    let mut cache = FormTextureCache::new();
    let asset = form_mesh_asset_with_textures(&geometry, ShadingModel::Layered, &mut cache).unwrap();
    assert_eq!(cache.len(), 1);

    // The painted slot samples the map and alpha-tests; the default slot does neither.
    let painted = asset.materials().iter().find(|material| material.material().name == "Painted").unwrap();
    assert!(painted.texture("baseColor").is_some());
    assert_eq!(painted.material().alpha_mode, AlphaMode::Mask);
    assert_eq!(asset.submeshes().len(), 2, "each used slot draws its own range");

    // Rebuilding reuses the decoded image.
    let again = form_mesh_asset_with_textures(&geometry, ShadingModel::Layered, &mut cache).unwrap();
    let map = |asset: &MeshAsset| {
        asset.materials().iter().find_map(|material| material.texture("baseColor").cloned()).unwrap()
    };
    assert!(map(&asset).ptr_eq(&map(&again)));

    let broken = palette.to_string().replace("UklGRh4A", "iVBORw0K");
    let geometry = joy_form::compile(source, &json!({}), Some(&serde_json::from_str(&broken).unwrap())).unwrap();
    assert!(matches!(form_mesh_asset(&geometry, ShadingModel::Layered), Err(RenderError::InvalidTexture(_))));
    assert!(form_mesh_asset(&geometry, ShadingModel::Lit).is_ok(), "flat assets ignore materials and their maps");
    assert!(matches!(form_mesh_asset(&geometry, ShadingModel::Custom("toon")), Err(RenderError::InvalidMaterial(_))));
}

#[test]
fn invalid_source_reports_a_located_diagnostic() {
    let error = compile_form_mesh("box(", &json!({}), None, ShadingModel::Lit).unwrap_err();
    assert!(matches!(error, FormMeshError::Form(diagnostic) if diagnostic.line == 1));
}
