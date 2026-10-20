// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/level_scene.rs`.

use joy_core::math::Vec3;
use serde_json::json;

use super::*;

fn entity(value: serde_json::Value) -> EntityDefinition {
    EntityDefinition::from_json(&value).unwrap()
}

#[test]
fn entities_become_scaled_tinted_instances_and_lights() {
    let entities = [
        entity(json!({"id": "crate", "transform": {"position": [1, 0, 0], "scale": [2, 1, 1]},
            "components": {"render": {"shape": "box", "size": [1, 2, 3], "color": [0.2, 0.4, 0.6]}}})),
        entity(json!({"id": "lamp", "transform": {"position": [0, 3, 0]},
            "components": {"light": {"color": [1, 0.5, 0], "intensity": 2, "range": 6}}})),
        entity(json!({"id": "marker", "components": {"render": {"shape": "plane"}}})),
    ];
    let mut scene = RenderScene::new();
    let mut level = LevelScene::new();
    level.sync(&mut scene, &entities, None).unwrap();

    assert_eq!(scene.len(), 1);
    let instance = scene.get(level.instance("crate").unwrap()).unwrap();
    assert_eq!(instance.transform.scale, Vec3::new(2.0, 2.0, 3.0));
    assert_eq!(instance.tint, LinearColor::rgb(0.2, 0.4, 0.6));
    assert!(instance.asset.ptr_eq(level.box_asset()));
    assert_eq!(level.lights().len(), 1);
    assert_eq!(level.lights()[0].color, LinearColor::rgb(2.0, 1.0, 0.0));
    assert_eq!(level.lights()[0].position, Vec3::new(0.0, 3.0, 0.0));

    level.sync(&mut scene, &entities, Some("crate")).unwrap();
    let highlighted = scene.get(level.instance("crate").unwrap()).unwrap().tint;
    assert!((highlighted.red - 0.48).abs() < 1e-6);

    level.sync(&mut scene, &entities[1..], None).unwrap();
    assert!(scene.is_empty());
}

#[test]
fn shape_changes_replace_instances_and_invalid_records_leave_the_scene_unchanged() {
    let mut scene = RenderScene::new();
    let mut level = LevelScene::new();
    level.sync(&mut scene, &[entity(json!({"id": "a", "components": {"render": {"shape": "box"}}}))], None).unwrap();
    let first = level.instance("a").unwrap();
    level.sync(&mut scene, &[entity(json!({"id": "a", "components": {"render": {"shape": "sphere"}}}))], None).unwrap();
    let second = level.instance("a").unwrap();
    assert_ne!(first, second);
    assert!(scene.get(first).is_none());
    assert!(scene.get(second).unwrap().asset.ptr_eq(level.sphere_asset()));

    let broken = entity(json!({"id": "b", "components": {"light": {"range": -1}}}));
    let error = level.sync(&mut scene, &[broken], None).unwrap_err();
    assert!(error.to_string().starts_with("Entity b:"));
    assert!(scene.get(second).is_some());
}

#[test]
fn mesh_resolvers_supply_assets_and_fall_back_to_shapes() {
    let entities =
        [entity(json!({"id": "kart", "components": {"render": {"asset": "assets/kart.form", "shape": "box"}}}))];
    let mut scene = RenderScene::new();
    let mut level = LevelScene::new();
    level.sync(&mut scene, &entities, None).unwrap();
    let fallback = scene.get(level.instance("kart").unwrap()).unwrap();
    assert!(fallback.asset.ptr_eq(level.box_asset()));
    assert_eq!(fallback.tint, RENDER_COMPONENT_DEFAULTS.primitive_color);

    let custom = primitive_sphere();
    level.sync_with(&mut scene, &entities, None, |_, _| Ok(Some(custom.clone()))).unwrap();
    let instance = scene.get(level.instance("kart").unwrap()).unwrap();
    assert!(instance.asset.ptr_eq(&custom));
    assert_eq!(instance.tint, LinearColor::WHITE);
}
