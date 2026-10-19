// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Presentation component schemas and readers: the recorded `render`, `light`, and `effect` schemas, presets,
//! validation issues, and field lookups, the `light` and `render` readers, plus strict `postProcess` reading and
//! preset round trips through the engine component catalog.

use joy_core::math::Vec3;
use joy_core::{
    LinearColor,
    PrimitiveShape,
};
use joy_gameplay::{
    EntityDefinition,
    LevelData,
    component_preset,
    component_type,
    parse_level,
    serialize_level,
};
use joy_scene::{
    ComponentError,
    LightComponent,
    PostProcessComponent,
    PostProcessShape,
    RENDER_COMPONENT_DEFAULTS,
    RenderComponent,
    RenderSource,
    engine_component_catalog,
    presentation_component_schemas,
};
use serde_json::{
    Value,
    json,
};

use joy_test_support::assert_matches_recording;

const REFERENCE: &str = include_str!("../fixtures/presentation-components.json");

fn reference() -> Value {
    serde_json::from_str(REFERENCE).expect("the presentation component reference is valid JSON")
}

#[test]
fn presentation_schemas_presets_and_issues_match_the_recording() {
    let reference = reference();
    let recorded_types = ["render", "light", "effect"];
    let schemas: Vec<_> = presentation_component_schemas()
        .into_iter()
        .filter(|schema| recorded_types.contains(&schema.kind.as_str()))
        .collect();
    assert_matches_recording(&schemas, &reference["builtIns"], "presentation schemas");

    let catalog = engine_component_catalog();
    for preset in reference["presets"].as_array().unwrap() {
        let created = catalog.create_component(preset["id"].as_str().unwrap()).unwrap();
        assert_matches_recording(&created, &preset["created"], preset["id"].as_str().unwrap());
    }
    for case in reference["validationCases"].as_array().unwrap() {
        let issues = catalog.validate(case["type"].as_str().unwrap(), &case["data"]);
        assert_matches_recording(&issues, &case["issues"], &case["data"].to_string());
    }
    for case in reference["entityCases"].as_array().unwrap() {
        let entity = EntityDefinition::from_json(&case["entity"]).unwrap();
        assert_matches_recording(&catalog.validate_entity(&entity), &case["issues"], &entity.id);
    }
    for case in reference["fieldCases"].as_array().unwrap() {
        let path: Vec<&str> =
            case["path"].as_array().unwrap().iter().map(|segment| segment.as_str().unwrap()).collect();
        let field = catalog.field(case["type"].as_str().unwrap(), &path);
        let actual =
            json!({"name": field.map(|field| &field.name), "unit": field.and_then(|field| field.unit.as_ref())});
        assert_matches_recording(&actual, &json!({"name": case["field"], "unit": case["unit"]}), &case.to_string());
    }
}

#[test]
fn engine_presets_validate_round_trip_and_read() {
    let catalog = engine_component_catalog();

    // Only unassigned required assets are reported; every other preset value satisfies its schema.
    let mut reported = Vec::new();
    for schema in catalog.list() {
        for preset in &schema.presets {
            let created = catalog.create_component(&preset.id).unwrap();
            for issue in catalog.validate(&created.kind, &Value::Object(created.data)) {
                reported.push(format!("{}.{}", issue.component, issue.field.unwrap_or_default()));
            }
        }
    }
    assert_eq!(reported, ["effect.asset"]);

    // A primitive kart, a lamp, and a bounded look survive a document round trip and read back.
    let data = |preset: &str| Value::Object(catalog.create_component(preset).unwrap().data);
    let entity = |id: &str, components: Value| {
        EntityDefinition::from_json(&json!({"id": id, "transform": {"position": [0, 1, 0]}, "components": components}))
            .unwrap()
    };
    let kart = entity(
        "kart",
        json!({"render": data(component_preset::RENDER_PRIMITIVE), "physics": data(component_preset::PHYSICS_BOX)}),
    );
    let lamp = entity("lamp", json!({"light": data(component_preset::LIGHT_POINT)}));
    let look = entity("look", json!({"postProcess": data(component_preset::POST_PROCESS_SPHERE)}));
    let level = LevelData { entities: vec![kart, lamp, look], ..LevelData::new("Presets") };
    let level = parse_level(&serialize_level(&level)).unwrap();
    assert!(level.entities.iter().all(|entity| catalog.validate_entity(entity).is_empty()));

    let render = RenderComponent::read(level.entities[0].components.get(component_type::RENDER).unwrap()).unwrap();
    assert!(matches!(render.source, RenderSource::Primitive(_)));
    let light = LightComponent::read(level.entities[1].components.get(component_type::LIGHT).unwrap()).unwrap();
    assert_eq!(light.range, 12.0);
    let look = PostProcessComponent::read(level.entities[2].components.get(component_type::POST_PROCESS).unwrap());
    assert!(matches!(look.unwrap().shape, PostProcessShape::Sphere { .. }));
}

#[test]
fn post_process_inspector_validation_agrees_with_the_volume_reader() {
    let catalog = engine_component_catalog();
    let records = [
        json!({"exposure": 2, "toneMapper": "aces"}),
        json!({"exposure": -1}),
        json!({"bloomKnee": 0}),
        json!({"vignetteRadius": 1.5}),
        json!({"toneMapper": "filmic"}),
        json!({"weight": 1.5}),
        json!({"shape": "sphere", "radius": 0}),
        json!({"shape": "box", "halfExtents": [1, 2, 3], "blendDistance": 0.5}),
    ];
    for record in records {
        let inspector_accepts = catalog.validate(component_type::POST_PROCESS, &record).is_empty();
        let reader_accepts = PostProcessComponent::read(record.as_object().unwrap()).is_ok();
        assert_eq!(inspector_accepts, reader_accepts, "{record}");
    }
}

#[test]
fn light_records_validate_only_when_enabled() {
    let read = |record: Value| LightComponent::read(record.as_object().unwrap());

    let light = read(json!({"intensity": 4, "castsShadow": false})).unwrap();
    assert_eq!(light.radiance(), LinearColor::rgb(4.0, 4.0, 4.0));
    assert!(!light.casts_shadow);
    assert_eq!(read(json!({"range": 0})), Err(ComponentError::InvalidLight));
    assert_eq!(read(json!({"intensity": null})), Err(ComponentError::NonFiniteLightValue));

    // Disabled lights skip validation.
    let disabled = read(json!({"enabled": false, "range": -1})).unwrap();
    assert!(!disabled.enabled);
}

#[test]
fn render_records_follow_scene_rules() {
    let read = |record: Value| RenderComponent::read(record.as_object().unwrap());

    let primitive = read(json!({"shape": "sphere", "size": [2, 1, 2]})).unwrap();
    assert_eq!(primitive.source, RenderSource::Primitive(PrimitiveShape::Sphere));
    assert_eq!(primitive.size, Vec3::new(2.0, 1.0, 2.0));
    assert_eq!(primitive.color, RENDER_COMPONENT_DEFAULTS.primitive_color);

    // Meshes take the mesh tint; a malformed size falls back to the default instead of failing the entity.
    let mesh = read(json!({"asset": "assets/kart.form", "size": "big"})).unwrap();
    assert!(matches!(mesh.source, RenderSource::Mesh { ref asset, .. } if asset == "assets/kart.form"));
    assert_eq!(mesh.size, RENDER_COMPONENT_DEFAULTS.size);
    assert_eq!(mesh.color, RENDER_COMPONENT_DEFAULTS.mesh_tint);

    assert_eq!(read(json!({"shape": "plane"})).unwrap().source, RenderSource::Hidden);
    assert!(read(json!({"asset": "assets/kart.png"})).is_err());
    assert_eq!(read(json!({"asset": "assets/a.form", "parameters": 4})), Err(ComponentError::InvalidParameters));
}

#[test]
fn post_process_records_are_read_strictly() {
    let read = |record: Value| PostProcessComponent::read(record.as_object().unwrap());

    // Volume fields and settings separate; omitted fields take the component defaults.
    let volume = read(json!({"shape": "box", "halfExtents": [2, 1, 3], "blendDistance": 0.5, "exposure": 2})).unwrap();
    assert_eq!(volume.shape, PostProcessShape::Box { half_extents: Vec3::new(2.0, 1.0, 3.0) });
    assert!(volume.enabled && !volume.unbounded);
    assert_eq!((volume.weight, volume.blend_distance), (1.0, 0.5));
    assert_eq!(volume.overrides.record().keys().collect::<Vec<_>>(), ["exposure"]);

    // Typos, bad shapes, and out-of-range values fail rather than silently doing nothing.
    assert!(matches!(read(json!({"glow": 1})), Err(ComponentError::PostProcess(_))));
    assert!(matches!(read(json!({"exposure": -1})), Err(ComponentError::PostProcess(_))));
    assert!(read(json!({"shape": "cone"})).is_err());
    assert!(read(json!({"weight": 1.5})).is_err());
    assert!(read(json!({"blendDistance": -1})).is_err());
    assert!(read(json!({"halfExtents": [1, 0, 1]})).is_err());
    assert!(read(json!({"shape": "sphere", "radius": 0})).is_err());
    assert!(read(json!({"unbounded": "yes"})).is_err());
}
