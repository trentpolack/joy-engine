// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/entity_definition.rs`.

use super::*;
use serde_json::json;

#[test]
fn defaults_and_validation_match_copy_entity_definition() {
    let definition = EntityDefinition::from_json(&json!({"id": "crate", "unknown": true})).unwrap();
    assert_eq!(definition.name, "crate");
    assert_eq!(definition.kind, entity_type::ENTITY);
    assert_eq!(definition.transform, EntityTransform::IDENTITY);
    assert_eq!(
        definition.to_json().as_object().unwrap().keys().collect::<Vec<_>>(),
        ["id", "name", "type", "tags", "transform", "components"]
    );

    let reject = |value: Value| EntityDefinition::from_json(&value).unwrap_err();
    assert_eq!(reject(json!({"id": "  "})), DefinitionError::InvalidId);
    assert_eq!(reject(json!({"id": "a", "name": null})), DefinitionError::InvalidNameOrType);
    assert_eq!(reject(json!({"id": "a", "tags": [1]})), DefinitionError::InvalidTags);
    assert_eq!(reject(json!({"id": "a", "components": {"data": 3}})), DefinitionError::InvalidComponent("data".into()));
    assert_eq!(
        reject(json!({"id": "a", "transform": {"position": [0, 1]}})),
        DefinitionError::InvalidVector("position")
    );
    assert_eq!(
        reject(json!({"id": "a", "transform": {"scale": [0, 1e39, 0]}})),
        DefinitionError::InvalidVector("scale")
    );
    assert_eq!(reject(json!({"id": "a", "overrides": {}})), DefinitionError::InvalidOverrides);
    assert_eq!(
        reject(json!({"id": "a", "template": "assets/a.joyobject", "overrides": {"name": "x"}})),
        DefinitionError::UnsupportedOverride
    );
    assert!(matches!(reject(json!({"id": "a", "template": "../a.joyobject"})), DefinitionError::AssetPath(_)));
}

#[test]
fn numbers_serialize_like_javascript() {
    assert_eq!(number_to_json(1.0).to_string(), "1");
    assert_eq!(number_to_json(-0.0).to_string(), "0");
    assert_eq!(number_to_json(0.1).to_string(), "0.1");
    assert_eq!(number_to_json(-0.25).to_string(), "-0.25");
    assert_eq!(number_to_json(f32::NAN), Value::Null);
}
