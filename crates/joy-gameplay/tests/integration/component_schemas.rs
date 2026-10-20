// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Component schema catalog checked against `tests/fixtures/gameplay-reference.json`, a golden recording of the
//! built-in schemas and presets, validation issue paths and messages, conditional fields, nested field lookup, and
//! registration errors. Comparisons are exact, and each test checks and re-records only its own sections of the shared
//! file. Re-record with `JOY_UPDATE_FIXTURES=1` when the catalog changes on purpose.

use joy_gameplay::{
    ComponentSchema,
    ComponentSchemaCatalog,
    EntityDefinition,
    EntityId,
    EntityRegistry,
    LevelData,
    LevelRuntime,
    built_in_component_schemas,
    component_preset,
    component_type,
    is_component_field_active,
    parse_level,
    serialize_level,
};
use joy_test_support::{
    GoldenFile,
    Tolerance,
    golden_file,
};
use serde_json::{
    Value,
    json,
};

/// The golden recording these tests share with the level document history test.
fn golden() -> GoldenFile {
    golden_file!("gameplay-reference.json")
}

/// A result as the recording spells it: `{"ok": value}` or `{"error": message}`.
fn outcome_record<T: serde::Serialize, E: std::fmt::Display>(result: Result<T, E>) -> Value {
    match result {
        Ok(value) => json!({"ok": value}),
        Err(error) => json!({"error": error.to_string()}),
    }
}

/// Replay the recorded cases at `pointer`: `record_outputs` replaces each case's outputs with what the code produces
/// now, and the section is checked exactly (or re-recorded).
fn check_cases(golden: &GoldenFile, pointer: &str, record_outputs: impl Fn(&mut Value)) {
    let mut cases = golden.recording().pointer(pointer).unwrap_or_else(|| panic!("no recorded {pointer}")).clone();
    for case in cases.as_array_mut().unwrap() {
        record_outputs(case);
    }
    golden.assert_section_matches(pointer, &cases, &Tolerance::exact());
}

#[test]
fn built_in_schemas_and_presets_match_the_recording() {
    let golden = golden();
    let catalog = ComponentSchemaCatalog::new();
    golden.assert_section_matches("/builtIns", &json!(built_in_component_schemas()), &Tolerance::exact());

    // Every catalog preset, in display order, with the component record it creates.
    let presets: Vec<Value> = catalog
        .list()
        .flat_map(|schema| &schema.presets)
        .map(|preset| json!({"id": preset.id, "created": catalog.create_component(&preset.id).unwrap()}))
        .collect();
    golden.assert_section_matches("/presets", &Value::Array(presets), &Tolerance::exact());

    let unknown = outcome_record(catalog.create_component("physics.nothing"));
    golden.assert_section_matches("/unknownPreset", &unknown, &Tolerance::exact());
}

#[test]
fn validation_reports_the_recorded_paths_and_messages() {
    let golden = golden();
    let catalog = ComponentSchemaCatalog::new();
    check_cases(&golden, "/validationCases", |case| {
        case["issues"] = json!(catalog.validate(case["type"].as_str().unwrap(), &case["data"]));
    });
    check_cases(&golden, "/entityCases", |case| {
        let entity = EntityDefinition::from_json(&case["entity"]).unwrap();
        case["issues"] = json!(catalog.validate_entity(&entity));
    });
}

#[test]
fn field_lookup_and_conditional_activity_match_the_recording() {
    let golden = golden();
    let catalog = ComponentSchemaCatalog::new();
    check_cases(&golden, "/fieldCases", |case| {
        let path: Vec<&str> =
            case["path"].as_array().unwrap().iter().map(|segment| segment.as_str().unwrap()).collect();
        let field = catalog.field(case["type"].as_str().unwrap(), &path);
        case["field"] = json!(field.map(|field| &field.name));
        case["unit"] = json!(field.and_then(|field| field.unit.as_ref()));
    });

    let physics = catalog.get(component_type::PHYSICS).unwrap();
    check_cases(&golden, "/activeCases", |case| {
        let record = case["record"].as_object().unwrap();
        let active: Vec<bool> = physics.fields.iter().map(|field| is_component_field_active(field, record)).collect();
        case["active"] = json!(active);
    });
}

#[test]
fn invalid_project_schemas_fail_registration_without_changing_the_catalog() {
    let golden = golden();
    check_cases(&golden, "/registrationCases", |case| {
        let schema: ComponentSchema = serde_json::from_value(case["schema"].clone()).unwrap();
        let mut catalog = ComponentSchemaCatalog::new();
        let registered = catalog.register(schema).map(|_| true);
        if registered.is_err() {
            assert_eq!(
                catalog,
                ComponentSchemaCatalog::new(),
                "{}: rejected schemas leave the catalog unchanged",
                case["name"]
            );
        }
        case["result"] = outcome_record(registered);
    });

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

    // Every simulation preset satisfies its own schema.
    for schema in catalog.list() {
        for preset in &schema.presets {
            let created = catalog.create_component(&preset.id).unwrap();
            assert!(catalog.validate(&created.kind, &Value::Object(created.data)).is_empty(), "{}", preset.id);
        }
    }

    // A collider and vehicle kart survives a document round trip and spawns.
    let data = |preset: &str| Value::Object(catalog.create_component(preset).unwrap().data);
    let mut physics = data(component_preset::PHYSICS_BOX);
    physics["mass"] = json!(120);
    let kart = EntityDefinition::from_json(&json!({
        "id": "kart",
        "transform": {"position": [0, 1, 0]},
        "components": {"physics": physics, "vehicle": data(component_preset::VEHICLE_FOUR_WHEEL)}
    }))
    .unwrap();
    let level = LevelData { entities: vec![kart], ..LevelData::new("Presets") };
    let level = parse_level(&serialize_level(&level)).unwrap();
    assert!(level.entities.iter().all(|entity| catalog.validate_entity(entity).is_empty()));
    let runtime = LevelRuntime::new(&level, EntityRegistry::new()).unwrap();
    assert!(runtime.body(&EntityId::from("kart")).is_some_and(|body| runtime.physics().is_vehicle(body)));
    assert!(level.entities[0].components.contains_key(component_type::VEHICLE));
}
