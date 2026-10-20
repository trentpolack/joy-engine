// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Tuning config values and their schema checked against `tests/fixtures/core-reference.json`, a golden recording of
//! merged values, rejection messages, and pretty-printed schema text for a set of authored fields. Comparisons are
//! exact. Re-record with `JOY_UPDATE_FIXTURES=1` when config validation or the schema changes on purpose.

use indexmap::IndexMap;
use joy_core::{
    ConfigApplyMode,
    ConfigField,
    create_config_document_values,
    create_config_schema,
    create_config_values,
};
use joy_test_support::{
    Tolerance,
    compare,
    golden_file,
};
use serde_json::{
    Value,
    json,
};

/// Deserialize recorded fields; JSON cannot spell `Infinity`, so the recording writes it as `null`, and `null` numbers
/// read back as infinity.
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

/// Recorded overrides; `null` stands for infinity.
fn overrides(value: &Value) -> IndexMap<String, f64> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, entry)| (key.clone(), entry.as_f64().unwrap_or(f64::INFINITY)))
        .collect()
}

/// A result as the recording spells it: `{"ok": values}` or `{"error": message}`.
fn outcome_record<T: serde::Serialize, E: std::fmt::Display>(result: Result<T, E>) -> Value {
    match result {
        Ok(values) => json!({"ok": serde_json::to_value(values).expect("config values serialize")}),
        Err(error) => json!({"error": error.to_string()}),
    }
}

/// The recorded cases with each case's `result` replaced by what `result_of` produces for it now.
fn cases_with_results(recorded: &Value, result_of: impl Fn(&Value) -> Value) -> Value {
    let mut cases = recorded.clone();
    for case in cases.as_array_mut().unwrap() {
        case["result"] = result_of(case);
    }
    cases
}

/// The full schema's recorded text, kept while it still parses to exactly the schema the code produces, or the
/// produced schema's pretty-printed text once it differs.
///
/// serde_json writes `1e-6` where the recording has `0.000001`; keeping text that still agrees mirrors how golden files
/// keep the recorded spelling of numbers that have not changed.
fn schema_text_record(schema: &Value, recorded_text: &Value) -> Value {
    let recorded_schema: Option<Value> = recorded_text.as_str().and_then(|text| serde_json::from_str(text).ok());
    let unchanged = recorded_schema.is_some_and(|recorded| compare(schema, &recorded, &Tolerance::exact()).is_empty());
    if unchanged {
        return recorded_text.clone();
    }
    Value::from(serde_json::to_string_pretty(schema).expect("schemas serialize"))
}

#[test]
fn config_values_merge_and_reject_as_recorded() {
    let golden = golden_file!("core-reference.json");
    let recording = golden.recording();
    let fields = fields(&recording["fields"]);
    assert_eq!(fields[2].apply_mode, ConfigApplyMode::Restart);
    assert_eq!(fields[3].apply_mode, ConfigApplyMode::NextSpawn);

    // Overrides, override documents, and invalid field lists each replay against the recorded fields.
    let value_cases = cases_with_results(&recording["valueCases"], |case| {
        outcome_record(create_config_values(&fields, &overrides(&case["overrides"])))
    });
    let document_cases = cases_with_results(&recording["documentCases"], |case| {
        outcome_record(create_config_document_values(&fields, &case["document"]))
    });
    let field_cases = cases_with_results(&recording["fieldCases"], |case| {
        outcome_record(create_config_values(&self::fields(&case["fields"]), &IndexMap::new()))
    });
    golden.assert_section_matches("/valueCases", &value_cases, &Tolerance::exact());
    golden.assert_section_matches("/documentCases", &document_cases, &Tolerance::exact());
    golden.assert_section_matches("/fieldCases", &field_cases, &Tolerance::exact());

    // Values keep field order, so typed tuning views can read them positionally.
    let values = create_config_values(&fields, &IndexMap::new()).unwrap();
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        fields.iter().map(|field| field.key.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn config_schema_serializes_to_the_recorded_text() {
    let golden = golden_file!("core-reference.json");
    let recording = golden.recording();
    let fields = fields(&recording["fields"]);
    let schema = create_config_schema(&fields).unwrap();
    let schema_text = schema_text_record(&schema, &recording["schemaText"]);
    golden.assert_section_matches("/schemaText", &schema_text, &Tolerance::exact());

    // Without the sub-1e-6 bounds, the schema serializes to the recorded text byte for byte.
    let ordinary: Vec<ConfigField> = fields.into_iter().filter(|field| field.key != "tiny").collect();
    let text = serde_json::to_string_pretty(&create_config_schema(&ordinary).unwrap()).unwrap();
    golden.assert_section_matches("/ordinarySchemaText", &Value::from(text), &Tolerance::exact());
}
