// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/material.rs`.

use super::*;

#[test]
fn materials_round_trip_with_model_tags_and_gltf_alpha_spellings() {
    let layer = SurfaceLayer {
        coat: Some(Coat::default()),
        maps: SurfaceMaps {
            normal: Some("data:image/png;base64,AA==".into()),
            ..SurfaceMaps::default()
        },
        ..SurfaceLayer::metallic_roughness(LinearColor::rgb(0.8, 0.2, 0.1), 0.25, 0.4)
    };
    let material = Material::layered(layer).with_name("paint").with_alpha_mode(AlphaMode::Mask);
    let json = serde_json::to_value(&material).unwrap();
    assert_eq!(json["alphaMode"], "MASK");
    assert_eq!(json["shading"]["layered"]["layer"]["f0"], serde_json::json!([0.04, 0.04, 0.04]));
    assert_eq!(serde_json::from_value::<Material>(json).unwrap(), material);

    // Omitted fields take their defaults, so hand-authored documents stay short.
    let lit: Material = serde_json::from_value(serde_json::json!({"shading": {"lit": {}}})).unwrap();
    assert_eq!(lit, Material::default());
    let custom: Material = serde_json::from_value(serde_json::json!({
        "shading": {"custom": {"model": "toon", "parameters": {"bands": 3.0, "rim": [1.0, 0.5, 0.25, 1.0]}}}
    }))
    .unwrap();
    assert_eq!(custom.model(), ShadingModel::Custom("toon"));
    assert_eq!(custom.validate(), Ok(()));
}

#[test]
fn validation_rejects_values_shaders_cannot_use() {
    let invalid = |layer: SurfaceLayer| Material::layered(layer).validate();
    assert_eq!(
        invalid(SurfaceLayer {
            roughness: 1.5,
            ..SurfaceLayer::default()
        }),
        Err(MaterialError::OutOfRange("roughness"))
    );
    assert_eq!(
        invalid(SurfaceLayer {
            base_color: LinearColor::rgb(-1.0, 0.0, 0.0),
            ..SurfaceLayer::default()
        }),
        Err(MaterialError::InvalidColor("baseColor"))
    );
    assert_eq!(
        invalid(SurfaceLayer {
            thin_film: Some(ThinFilm {
                thickness: 5000.0,
                ior: 1.3,
                weight: 1.0
            }),
            ..SurfaceLayer::default()
        }),
        Err(MaterialError::OutOfRange("thinFilm.thickness"))
    );
    let empty_map = SurfaceLayer {
        maps: SurfaceMaps {
            base_color: Some(" ".into()),
            ..SurfaceMaps::default()
        },
        ..SurfaceLayer::default()
    };
    assert_eq!(invalid(empty_map), Err(MaterialError::EmptyTextureSource));
    let unnamed = Material::new(Shading::Custom(CustomShading::default()));
    assert_eq!(unnamed.validate(), Err(MaterialError::UnnamedCustomModel));
}

#[test]
fn operators_collapse_into_one_equivalent_layer() {
    let red = SurfaceLayer::dielectric(LinearColor::rgb(1.0, 0.0, 0.0), 0.2);
    let gold = SurfaceLayer::metal(LinearColor::rgb(1.0, 0.8, 0.3), 0.6);

    // A blend interpolates parameters and keeps the heavier side's maps.
    let blend = Surface::Blend {
        first: Box::new(Surface::Layer(red.clone())),
        second: Box::new(Surface::Layer(gold.clone())),
        weight: 0.25,
    }
    .collapse();
    assert!((blend.metallic - 0.25).abs() < 1e-6);
    assert!((blend.roughness - 0.3).abs() < 1e-6);
    assert!((blend.base_color.green - 0.2).abs() < 1e-6);

    // A coat keeps the base's reflectance and turns the upper surface into the coat lobe.
    let lacquer = SurfaceLayer {
        roughness: 0.05,
        ..SurfaceLayer::dielectric(LinearColor::WHITE, 0.05)
    };
    let coated = Surface::Coat {
        coat: Box::new(Surface::Layer(lacquer)),
        base: Box::new(Surface::Layer(gold.clone())),
    }
    .collapse();
    assert_eq!(coated.base_color, gold.base_color);
    let coat = coated.coat.unwrap();
    assert!((coat.roughness - 0.05).abs() < 1e-6 && (coat.f0 - DIELECTRIC_F0).abs() < 1e-6 && coat.weight == 1.0);

    // A weight scales coverage without touching reflectance.
    let weighted = Surface::Weight {
        surface: Box::new(Surface::Layer(red.clone())),
        weight: 0.5,
    }
    .collapse();
    assert_eq!(weighted.base_color, red.base_color.with_alpha(0.5));
}
