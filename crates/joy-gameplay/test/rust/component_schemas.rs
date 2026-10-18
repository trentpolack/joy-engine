// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Component schema catalog parity: built-in schemas, presets, validation issue paths and messages, conditional fields,
//! nested field lookup, and registration errors, compared exactly against the TypeScript engine.

use joy_gameplay::{
    ComponentSchema,
    ComponentSchemaCatalog,
    EntityDefinition,
    EntityId,
    EntityRegistry,
    LevelData,
    LevelRuntime,
    LightComponent,
    RenderComponent,
    RenderSource,
    built_in_component_schemas,
    component_preset,
    component_type,
    is_component_field_active,
    parse_level,
    serialize_level,
};
use serde_json::{
    Value,
    json,
};

use joy_test_support::assert_matches_recording;

const REFERENCE: &str = include_str!("../fixtures/gameplay-reference.json");

fn reference() -> Value {
    serde_json::from_str(REFERENCE).expect("the gameplay reference is valid JSON")
}

#[test]
fn built_in_schemas_and_presets_match_typescript() {
    let reference = reference();
    assert_matches_recording(&built_in_component_schemas(), &reference["builtIns"], "built-in schemas");

    let catalog = ComponentSchemaCatalog::new();
    let preset_ids: Vec<&str> =
        catalog.list().flat_map(|schema| schema.presets.iter().map(|preset| preset.id.as_str())).collect();
    let recorded: Vec<&str> =
        reference["presets"].as_array().unwrap().iter().map(|preset| preset["id"].as_str().unwrap()).collect();
    assert_eq!(preset_ids, recorded);
    for preset in reference["presets"].as_array().unwrap() {
        let created = catalog.create_component(preset["id"].as_str().unwrap()).unwrap();
        assert_matches_recording(&created, &preset["created"], preset["id"].as_str().unwrap());
    }
    let unknown = catalog.create_component("render.nothing").unwrap_err();
    assert_eq!(unknown.to_string(), reference["unknownPreset"]["error"].as_str().unwrap());
}

#[test]
fn validation_reports_typescript_paths_and_messages() {
    let reference = reference();
    let catalog = ComponentSchemaCatalog::new();
    for case in reference["validationCases"].as_array().unwrap() {
        let issues = catalog.validate(case["type"].as_str().unwrap(), &case["data"]);
        assert_matches_recording(&issues, &case["issues"], &case["data"].to_string());
    }
    for case in reference["entityCases"].as_array().unwrap() {
        let entity = EntityDefinition::from_json(&case["entity"]).unwrap();
        assert_matches_recording(&catalog.validate_entity(&entity), &case["issues"], &entity.id);
    }
}

#[test]
fn field_lookup_and_conditional_activity_match_typescript() {
    let reference = reference();
    let catalog = ComponentSchemaCatalog::new();
    for case in reference["fieldCases"].as_array().unwrap() {
        let path: Vec<&str> =
            case["path"].as_array().unwrap().iter().map(|segment| segment.as_str().unwrap()).collect();
        let field = catalog.field(case["type"].as_str().unwrap(), &path);
        let actual =
            json!({"name": field.map(|field| &field.name), "unit": field.and_then(|field| field.unit.as_ref())});
        assert_matches_recording(&actual, &json!({"name": case["field"], "unit": case["unit"]}), &case.to_string());
    }

    let physics = catalog.get(component_type::PHYSICS).unwrap();
    for case in reference["activeCases"].as_array().unwrap() {
        let record = case["record"].as_object().unwrap();
        let active: Vec<bool> = physics.fields.iter().map(|field| is_component_field_active(field, record)).collect();
        assert_matches_recording(&active, &case["active"], &case["record"].to_string());
    }
}

#[test]
fn invalid_project_schemas_fail_registration_without_changing_the_catalog() {
    let reference = reference();
    for case in reference["registrationCases"].as_array().unwrap() {
        let schema: ComponentSchema = serde_json::from_value(case["schema"].clone()).unwrap();
        let mut catalog = ComponentSchemaCatalog::new();
        let error = catalog.register(schema.clone()).map(|_| ()).unwrap_err();
        assert_eq!(error.to_string(), case["result"]["error"].as_str().unwrap(), "{}", case["name"]);
        assert_eq!(
            catalog,
            ComponentSchemaCatalog::new(),
            "{}: rejected schemas leave the catalog unchanged",
            case["name"]
        );
    }

    // Project schemas register beside built-ins; catalogs are independent and resolve nested fields.
    let mut project = ComponentSchemaCatalog::new();
    let spawner: ComponentSchema = serde_json::from_value(json!({
        "type": "spawner", "label": "Spawner", "description": "Spawns waves.", "category": "Gameplay",
        "fields": [{"name": "interval", "kind": "number", "unit": "s", "min": 0, "minExclusive": true, "default": 2}],
        "presets": [{"id": "spawner.default", "label": "Spawner", "data": {"interval": 2}}]
    }))
    .unwrap();
    project.register(spawner).unwrap();
    assert_eq!(project.get("spawner").unwrap().fields[0].unit.as_deref(), Some("s"));
    assert!(!ComponentSchemaCatalog::new().has("spawner"));
    assert!(ComponentSchemaCatalog::empty().list().next().is_none());
    let issues = project.validate("spawner", &json!({"interval": 0}));
    assert_eq!(issues[0].message, "Interval must be greater than 0.");
}

#[test]
fn preset_records_round_trip_and_run_in_the_engine_runtimes() {
    let catalog = ComponentSchemaCatalog::new();

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

    // A primitive, collider, and vehicle kart plus a lamp survive a document round trip and spawn.
    let data = |preset: &str| Value::Object(catalog.create_component(preset).unwrap().data);
    let mut physics = data(component_preset::PHYSICS_BOX);
    physics["mass"] = json!(120);
    let entity = |id: &str, components: Value| {
        EntityDefinition::from_json(&json!({"id": id, "transform": {"position": [0, 1, 0]}, "components": components}))
            .unwrap()
    };
    let kart = entity(
        "kart",
        json!({"render": data(component_preset::RENDER_PRIMITIVE), "physics": physics, "vehicle": data(component_preset::VEHICLE_FOUR_WHEEL)}),
    );
    let lamp = entity("lamp", json!({"light": data(component_preset::LIGHT_POINT)}));
    let level = LevelData { entities: vec![kart, lamp], ..LevelData::new("Presets") };
    let level = parse_level(&serialize_level(&level)).unwrap();
    assert!(level.entities.iter().all(|entity| catalog.validate_entity(entity).is_empty()));

    let render = RenderComponent::read(level.entities[0].components.get(component_type::RENDER).unwrap()).unwrap();
    assert!(matches!(render.source, RenderSource::Primitive(_)));
    let light = LightComponent::read(level.entities[1].components.get(component_type::LIGHT).unwrap()).unwrap();
    assert_eq!(light.range, 12.0);
    let runtime = LevelRuntime::new(&level, EntityRegistry::new()).unwrap();
    assert!(runtime.vehicle(&EntityId::from("kart")).is_some());
}
