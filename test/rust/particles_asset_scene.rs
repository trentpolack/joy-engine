// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! `AssetScene`: FORM meshes and per-entity effects with staged synchronization. Ported from the TypeScript
//! `test/asset-scene.test.mjs`, plus parity with a recorded TypeScript `AssetScene` run and the Rust additions
//! (textured effects and world-space particle lights).

use std::collections::HashMap;

use joy_engine::gameplay::EntityDefinition;
use joy_engine::particles::{
    DEFAULT_PARTICLE_TEXTURE,
    ParticleBatch,
    ParticleCameraBasis,
};
use joy_engine::rendering::{
    AssetScene,
    AssetSceneError,
    RenderScene,
    TransparentDraw,
};
use serde_json::{
    Value,
    json,
};

const BOX_FORM: &str = "project meta(name=\"Box\")\nbox(0, 0, 0, 2, 2, 2)";

fn entity(value: Value) -> EntityDefinition {
    EntityDefinition::from_json(&value).unwrap_or_else(|error| panic!("{error}"))
}

fn reader(files: &HashMap<String, String>) -> impl FnMut(&str) -> Result<String, String> + '_ {
    move |path: &str| files.get(path).cloned().ok_or_else(|| format!("missing {path}"))
}

fn fixture_text(file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test/fixtures").join(file);
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn asset_scene_matches_the_typescript_presentation() {
    let fixture: Value = serde_json::from_str(include_str!("../fixtures/particles-parity.json")).unwrap();
    let recorded = &fixture["assetScene"];
    let files: HashMap<String, String> = recorded["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(path, text)| (path.clone(), text.as_str().unwrap().to_owned()))
        .collect();
    let entities: Vec<EntityDefinition> =
        recorded["entities"].as_array().unwrap().iter().cloned().map(entity).collect();
    let axis = |name: &str| -> [f64; 3] {
        let values: Vec<f64> =
            recorded["camera"][name].as_array().unwrap().iter().map(|value| value.as_f64().unwrap()).collect();
        [values[0], values[1], values[2]]
    };
    let camera = ParticleCameraBasis { right: axis("right"), up: axis("up"), backward: axis("backward") };

    let mut scene = RenderScene::new();
    let mut assets = AssetScene::new();
    assert!(assets.sync(&mut scene, &entities, None, reader(&files)).unwrap());
    for frame in recorded["frames"].as_array().unwrap() {
        assets.step(frame["seconds"].as_f64().unwrap());
        let effects: Vec<Value> = assets
            .effects()
            .map(|(id, instance)| {
                json!({"id": id, "ticks": instance.effect().ticks(), "count": instance.effect().particles().len()})
            })
            .collect();
        assert_eq!(Value::Array(effects), frame["effects"]);
        assert_eq!(assets.error(), frame["error"].as_str().unwrap());

        let vertices = assets.particles(&camera);
        let expected = &frame["particles"];
        assert_eq!(vertices.len() as u64, expected["vertexCount"].as_u64().unwrap());
        let values: Vec<f64> = vertices
            .iter()
            .flat_map(|vertex| {
                vertex.position.iter().chain(&vertex.color).map(|value| f64::from(*value)).collect::<Vec<_>>()
            })
            .collect();
        let sum: f64 = values.iter().sum();
        let absolute: f64 = values.iter().map(|value| value.abs()).sum();
        let scale = 1.0 + expected["abs"].as_f64().unwrap();
        assert!((sum - expected["sum"].as_f64().unwrap()).abs() <= 2e-6 * scale, "sum {sum} vs {}", expected["sum"]);
        assert!((absolute - expected["abs"].as_f64().unwrap()).abs() <= 2e-6 * scale);
        for (actual, expected) in values.iter().zip(expected["first"].as_array().unwrap()) {
            let expected = expected.as_f64().unwrap();
            assert!((actual - expected).abs() <= 2e-6 * (1.0 + expected.abs()), "{actual} vs {expected}");
        }
    }
}

#[test]
fn meshes_are_shared_effects_are_per_instance_and_failed_loads_keep_the_scene() {
    let mut files = HashMap::from([
        ("assets/box.form".to_owned(), BOX_FORM.to_owned()),
        ("assets/fire.joyfx".to_owned(), fixture_text("fire.joyfx")),
    ]);
    let entities: Vec<EntityDefinition> = ["a", "b"]
        .iter()
        .map(|id| {
            entity(json!({"id": id, "components": {"render": {"asset": "assets/box.form"},
                "effect": {"asset": "assets/fire.joyfx", "seed": 7777, "loop": true}}}))
        })
        .collect();
    let mut scene = RenderScene::new();
    let mut assets = AssetScene::new();
    assert!(assets.sync(&mut scene, &entities, None, reader(&files)).unwrap());
    assert!(assets.mesh("a").unwrap().ptr_eq(assets.mesh("b").unwrap()));
    assert_eq!(scene.len(), 2);
    let ticks = |assets: &AssetScene, id: &str| assets.effect(id).unwrap().effect().ticks();
    assets.step(0.1);
    assert!(!assets.particles(&joy_engine::particles::PARTICLE_XY_BASIS).is_empty());
    assert_eq!(ticks(&assets, "a"), 7);

    // Unchanged settings keep effects running; a changed seed restarts only that effect.
    let mut reseeded = entities.clone();
    reseeded[1].components.get_mut("effect").unwrap().insert("seed".to_owned(), json!(5));
    assert!(assets.sync(&mut scene, &reseeded, Some("a"), reader(&files)).unwrap());
    assert_eq!(ticks(&assets, "a"), 7);
    assert_eq!(ticks(&assets, "b"), 1);
    assert_eq!(assets.effect("b").unwrap().effect().seed(), 5);

    // A broken mesh rejects the whole sync and keeps the running presentation.
    let mesh = assets.mesh("a").unwrap().clone();
    files.insert("assets/box.form".to_owned(), "broken(".to_owned());
    let error = assets.sync(&mut scene, &entities, None, reader(&files)).unwrap_err();
    assert!(matches!(error, AssetSceneError::Mesh { .. }), "{error}");
    assert!(assets.mesh("a").unwrap().ptr_eq(&mesh));
    assert_eq!(ticks(&assets, "a"), 7);

    // Missing files and invalid effect settings are reported with their source.
    let missing = vec![entity(json!({"id": "x", "components": {"effect": {"asset": "assets/none.joyfx"}}}))];
    let error = assets.sync(&mut scene, &missing, None, reader(&files)).unwrap_err();
    assert_eq!(error.to_string(), "Could not read assets/none.joyfx: missing assets/none.joyfx");
    let unsafe_path = vec![entity(json!({"id": "x", "components": {"effect": {"asset": "../fire.joyfx"}}}))];
    assert!(matches!(
        assets.sync(&mut scene, &unsafe_path, None, reader(&files)),
        Err(AssetSceneError::Component { .. })
    ));
    let bad_seed =
        vec![entity(json!({"id": "x", "components": {"effect": {"asset": "assets/fire.joyfx", "seed": 1.5}}}))];
    let error = assets.sync(&mut scene, &bad_seed, None, reader(&files)).unwrap_err();
    assert_eq!(error.to_string(), "Entity x: Particle seed must be a finite integer.");
    assert_eq!(scene.len(), 2);

    assets.destroy(&mut scene);
    assert!(scene.is_empty());
    assert_eq!(assets.effects().count(), 0);
    assert!(matches!(assets.sync(&mut scene, &entities, None, reader(&files)), Err(AssetSceneError::Destroyed)));
}

#[test]
fn superseded_stages_cannot_replace_a_newer_scene_or_a_destroyed_owner() {
    let files = HashMap::from([("assets/box.form".to_owned(), BOX_FORM.to_owned())]);
    let mut scene = RenderScene::new();
    let mut assets = AssetScene::new();
    let old = assets
        .prepare(
            &[entity(json!({"id": "old", "components": {"render": {"asset": "assets/box.form"}}}))],
            reader(&files),
        )
        .unwrap();
    assert!(
        assets
            .sync(
                &mut scene,
                &[entity(json!({"id": "new", "components": {"render": {"shape": "box"}}}))],
                None,
                reader(&files)
            )
            .unwrap()
    );
    assert!(!assets.commit(&mut scene, old, None).unwrap());
    assert_eq!(assets.entities()[0].id, "new");

    let cancelled = assets.prepare(&[entity(json!({"id": "cancelled"}))], reader(&files)).unwrap();
    assets.cancel_pending();
    assert!(!assets.commit(&mut scene, cancelled, None).unwrap());
    assert_eq!(assets.entities()[0].id, "new");

    let late = assets.prepare(&[entity(json!({"id": "late"}))], reader(&files)).unwrap();
    assets.destroy(&mut scene);
    assert!(!assets.commit(&mut scene, late, None).unwrap());
    assert!(scene.is_empty());
}

#[test]
fn textured_effects_draw_and_particle_lights_follow_entities() {
    let files = HashMap::from([("assets/lantern.joyfx".to_owned(), fixture_text("joyfx/lantern.joyfx"))]);
    let lantern = entity(
        json!({"id": "lantern", "transform": {"position": [10, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1]},
        "components": {"effect": {"asset": "assets/lantern.joyfx", "seed": 3, "parameters": {"power": 2}},
            "light": {"color": [1, 1, 1], "intensity": 1, "range": 4}}}),
    );
    let mut scene = RenderScene::new();
    let mut assets = AssetScene::new();
    assert!(assets.sync(&mut scene, &[lantern], None, reader(&files)).unwrap());
    for _ in 0..4 {
        assets.step(0.1);
    }
    let camera = joy_engine::particles::PARTICLE_XY_BASIS;
    let batches = assets.particle_batches(&camera).unwrap();
    assert!(batches.iter().any(|batch| batch.source() == Some(DEFAULT_PARTICLE_TEXTURE)));
    assert!(batches.iter().any(|batch| matches!(batch, ParticleBatch::Colored(_))));
    let draws = assets.transparent_draws(&batches).unwrap();
    assert_eq!(draws.len(), batches.len());
    assert!(draws.iter().any(|draw| matches!(draw, TransparentDraw::Textured { .. })));

    let selection = assets.particle_lights(32);
    assert!(!selection.lights.is_empty());
    assert!(selection.lights.iter().all(|light| light.x > 8.0), "lights follow the entity position");
    let lights = assets.lights();
    assert_eq!(lights[0].range, 4.0, "light components come first");
    assert_eq!(lights.len(), 1 + selection.lights.len().min(31));
    assert!(assets.particles(&camera).iter().all(|vertex| vertex.position[0] > 5.0));
}
