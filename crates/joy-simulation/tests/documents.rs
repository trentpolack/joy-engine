// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Level and object document round trips and rejection of invalid authored data.

use joy_simulation::gameplay::{
    DefinitionError, EntityDefinition, LevelData, LevelError, ObjectError, create_object_instance, parse_level,
    parse_object, resolve_level_objects, resolve_object_instance, serialize_level, serialize_object,
};
use serde_json::{Value, json};

/// Engine-owned fixture written by the TypeScript `serializeLevel`, so it is in canonical form.
const TEST_YARD: &str = include_str!("fixtures/test-yard.joylevel");

#[test]
fn level_round_trip_is_byte_identical_and_preserves_custom_components() {
    let level = parse_level(TEST_YARD).unwrap();
    assert_eq!(serialize_level(&level), TEST_YARD);
    assert_eq!(parse_level(&serialize_level(&level)).unwrap(), level);

    assert_eq!(level.name, "Engine Test Yard");
    assert_eq!(
        level.entities.iter().map(|entity| entity.id.as_str()).collect::<Vec<_>>(),
        ["ground", "crate", "kart", "lamp"]
    );
    let crate_entity = &level.entities[1];
    assert_eq!(crate_entity.kind, "prop");
    assert_eq!(crate_entity.tags, ["dynamic", "pushable"]);
    assert_eq!(
        Value::Object(crate_entity.components["spawner"].clone()),
        json!({"rate": 2, "label": "Crates", "nested": {"enabled": true, "list": [1, 2.5, "x", null]}})
    );
    let kart = &level.entities[2];
    assert_eq!(kart.components["vehicle"]["wheels"].as_array().unwrap().len(), 4);
    let profile = level.post_processing.as_ref().unwrap();
    assert_eq!(profile.preset, "soft");
    assert_eq!(profile.overrides.keys().collect::<Vec<_>>(), ["bloomStrength", "vignetteStrength"]);
}

#[test]
fn invalid_levels_are_rejected() {
    let base = || serde_json::from_str::<Value>(TEST_YARD).unwrap();
    let parse = |value: Value| parse_level(&value.to_string());

    let mut duplicate = base();
    duplicate["entities"][1]["id"] = json!("ground");
    assert_eq!(parse(duplicate), Err(LevelError::DuplicateEntityId("ground".into())));

    let mut version = base();
    version["version"] = json!(2);
    assert_eq!(parse(version), Err(LevelError::UnsupportedVersion));

    let mut unnamed = base();
    unnamed["name"] = json!("   ");
    assert_eq!(parse(unnamed), Err(LevelError::MissingName));

    let mut transform = base();
    transform["entities"][0]["transform"]["position"] = json!([0, "high", 0]);
    assert_eq!(parse(transform), Err(LevelError::Definition(DefinitionError::InvalidVector("position"))));

    let mut short = base();
    short["entities"][0]["transform"]["scale"] = json!([1, 1]);
    assert_eq!(parse(short), Err(LevelError::Definition(DefinitionError::InvalidVector("scale"))));

    let mut component = base();
    component["entities"][0]["components"]["render"] = json!([1, 2, 3]);
    assert_eq!(parse(component), Err(LevelError::Definition(DefinitionError::InvalidComponent("render".into()))));

    let mut profile = base();
    profile["postProcessing"]["overrides"]["glow"] = json!(1);
    assert!(matches!(parse(profile), Err(LevelError::PostProcessing(_))));

    assert!(matches!(parse_level("{not json"), Err(LevelError::Json(_))));
    assert_eq!(parse(json!([])), Err(LevelError::NotAnObject));
    assert_eq!(parse(json!({"version": 1, "name": "x"})), Err(LevelError::MissingEntities));

    // Programmatic edits are re-validated on demand.
    let mut level = parse_level(TEST_YARD).unwrap();
    level.entities[0].transform.position.y = f32::NAN;
    assert!(level.validate().is_err());
}

/// The TypeScript suite's reusable object source.
fn vessel() -> Value {
    json!({
        "version": 1,
        "name": "Rain vessel",
        "entity": {
            "id": "object",
            "type": "vessel",
            "tags": ["wet"],
            "components": {
                "render": {"asset": "assets/vessel.form"},
                "gameplay": {"water": 10, "enabled": true, "custom": {"label": "Rain"}}
            }
        }
    })
}

#[test]
fn linked_objects_inherit_source_changes_while_local_overrides_and_identity_survive() {
    let object = parse_object(&vessel().to_string()).unwrap();
    let mut entity = create_object_instance(&object, "assets/vessel.joyobject", "vessel-1").unwrap();
    assert_eq!(entity.name, "Rain vessel");
    assert_eq!(entity.template.as_deref(), Some("assets/vessel.joyobject"));
    entity.overrides = Some(json!({"components": {"gameplay": {"water": 30}}}).as_object().cloned().unwrap());

    let mut changed = vessel();
    changed["entity"]["components"]["gameplay"]["enabled"] = json!(false);
    let resolved = resolve_object_instance(&entity, &parse_object(&changed.to_string()).unwrap()).unwrap();
    assert_eq!(resolved.id, "vessel-1");
    assert_eq!(resolved.kind, "vessel");
    assert_eq!(resolved.template, None);
    let gameplay = &resolved.components["gameplay"];
    assert_eq!(gameplay["water"], json!(30));
    assert_eq!(gameplay["enabled"], json!(false));
    assert_eq!(gameplay["custom"]["label"], json!("Rain"));

    // Linked instances round trip through a level unchanged.
    let mut level = LevelData::new("Garden");
    level.entities.push(entity.clone());
    assert_eq!(parse_level(&serialize_level(&level)).unwrap(), level);

    // Null component overrides remove the inherited component; the source is never modified.
    entity.overrides = Some(json!({"components": {"gameplay": null}}).as_object().cloned().unwrap());
    assert!(!resolve_object_instance(&entity, &object).unwrap().components.contains_key("gameplay"));
    assert_eq!(object.entity.components["gameplay"]["water"], json!(10));
    assert_eq!(parse_object(&serialize_object(&object).unwrap()).unwrap(), object);
}

#[test]
fn object_documents_reject_nested_links_unsafe_paths_and_malformed_overrides() {
    let mut nested = vessel();
    nested["entity"]["template"] = json!("assets/recursive.joyobject");
    assert_eq!(parse_object(&nested.to_string()), Err(ObjectError::NestedTemplate));

    let object = parse_object(&vessel().to_string()).unwrap();
    assert!(matches!(create_object_instance(&object, "../outside.joyobject", "x"), Err(ObjectError::AssetPath(_))));

    let mut unsupported =
        EntityDefinition::from_json(&json!({"id": "x", "template": "assets/vessel.joyobject"})).unwrap();
    unsupported.overrides = Some(json!({"name": "renamed"}).as_object().cloned().unwrap());
    assert_eq!(
        resolve_object_instance(&unsupported, &object),
        Err(ObjectError::Definition(DefinitionError::UnsupportedOverride))
    );
    assert_eq!(
        parse_object(&json!({"version": 1, "name": "", "entity": {"id": "a"}}).to_string()),
        Err(ObjectError::InvalidObject)
    );
}

#[test]
fn level_resolution_reads_each_source_once_and_leaves_the_authored_level_untouched() {
    let object = parse_object(&vessel().to_string()).unwrap();
    let mut level = LevelData::new("Garden");
    for id in ["a", "b"] {
        level.entities.push(create_object_instance(&object, "assets/vessel.joyobject", id).unwrap());
    }

    let mut reads = Vec::new();
    let resolved = resolve_level_objects(&level, |path| {
        reads.push(path.to_owned());
        Ok::<_, String>(vessel().to_string())
    })
    .unwrap();
    assert_eq!(reads, ["assets/vessel.joyobject"]);
    assert_eq!(resolved.entities[0].template, None);
    assert_eq!(resolved.entities[1].components["gameplay"]["water"], json!(10));
    assert_eq!(level.entities[0].template.as_deref(), Some("assets/vessel.joyobject"));

    let failure = resolve_level_objects(&level, |_| Err("missing file")).unwrap_err();
    assert_eq!(failure, ObjectError::Read { path: "assets/vessel.joyobject".into(), message: "missing file".into() });
}
