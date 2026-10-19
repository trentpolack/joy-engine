// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Foundation records against recorded expectations: tuning config values and their schema, and hex colors, compared
//! exactly.

use indexmap::IndexMap;
use joy_core::{
    ConfigApplyMode,
    ConfigField,
    create_config_document_values,
    create_config_schema,
    create_config_values,
};
use serde_json::Value;

use joy_test_support::assert_matches_recording;

const REFERENCE: &str = include_str!("../fixtures/core-reference.json");

fn reference() -> Value {
    serde_json::from_str(REFERENCE).expect("the core reference is valid JSON")
}

/// Deserialize recorded fields; JSON writes JavaScript `Infinity` as `null`, so `null` numbers read as infinity.
fn fields(value: &Value) -> Vec<ConfigField> {
    let mut value = value.clone();
    for field in value.as_array_mut().unwrap() {
        for (_, entry) in field.as_object_mut().unwrap().iter_mut() {
            if entry.is_null() {
                *entry = Value::from(f64::MAX);
            }
        }
    }
    let mut fields: Vec<ConfigField> = serde_json::from_value(value).unwrap();
    for field in &mut fields {
        for number in [&mut field.default_value, &mut field.min, &mut field.max, &mut field.step] {
            if *number == f64::MAX {
                *number = f64::INFINITY;
            }
        }
    }
    fields
}

/// Recorded overrides; `null` stands for JavaScript `Infinity`.
fn overrides(value: &Value) -> IndexMap<String, f64> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, entry)| (key.clone(), entry.as_f64().unwrap_or(f64::INFINITY)))
        .collect()
}

/// Compare a result with a recorded `{ok}` or `{error}` outcome.
fn assert_outcome<T: serde::Serialize, E: std::fmt::Display>(result: Result<T, E>, expected: &Value, context: &str) {
    match (result, expected.get("ok"), expected.get("error")) {
        (Ok(values), Some(ok), None) => assert_matches_recording(&values, ok, context),
        (Err(error), None, Some(message)) => assert_eq!(error.to_string(), message.as_str().unwrap(), "{context}"),
        (Ok(_), _, _) => panic!("{context}: expected {expected}, got success"),
        (Err(error), _, _) => panic!("{context}: expected {expected}, got error {error}"),
    }
}

#[test]
fn config_values_merge_and_reject_like_typescript() {
    let reference = reference();
    let fields = fields(&reference["fields"]);
    assert_eq!(fields[2].apply_mode, ConfigApplyMode::Restart);
    assert_eq!(fields[3].apply_mode, ConfigApplyMode::NextSpawn);

    for case in reference["valueCases"].as_array().unwrap() {
        let context = case["name"].as_str().unwrap();
        assert_outcome(create_config_values(&fields, &overrides(&case["overrides"])), &case["result"], context);
    }
    for case in reference["documentCases"].as_array().unwrap() {
        let context = case["name"].as_str().unwrap();
        assert_outcome(create_config_document_values(&fields, &case["document"]), &case["result"], context);
    }
    for case in reference["fieldCases"].as_array().unwrap() {
        let context = case["name"].as_str().unwrap();
        let invalid = self::fields(&case["fields"]);
        assert_outcome(create_config_values(&invalid, &IndexMap::new()), &case["result"], context);
    }

    // Values keep field order, so typed tuning views can read them positionally.
    let values = create_config_values(&fields, &IndexMap::new()).unwrap();
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        fields.iter().map(|field| field.key.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn config_schema_serializes_to_the_typescript_text() {
    let reference = reference();
    let fields = fields(&reference["fields"]);
    let schema = create_config_schema(&fields).unwrap();
    let expected = reference["schemaText"].as_str().unwrap();
    assert_matches_recording(&schema, &serde_json::from_str(expected).unwrap(), "schema value");

    // Byte-for-byte text, except for magnitudes below 1e-6 where serde_json and JavaScript spell numbers differently.
    let ordinary: Vec<ConfigField> = fields.into_iter().filter(|field| field.key != "tiny").collect();
    let text = serde_json::to_string_pretty(&create_config_schema(&ordinary).unwrap()).unwrap();
    assert_eq!(text, reference["ordinarySchemaText"].as_str().unwrap());
}
