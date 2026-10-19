// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/asset.rs`.

use super::*;
use serde_json::json;

fn document(emitter: Value) -> Value {
    let mut base = json!({"id": "main", "enabled": true, "root": true, "duration": 0, "rate": 0,
        "bursts": [{"time": 0, "count": 2}], "lifetime": 1, "renderer": "soft", "spawn": "", "update": ""});
    if let (Value::Object(base), Value::Object(overrides)) = (&mut base, emitter) {
        base.extend(overrides);
    }
    json!({"version": 1, "name": "Test", "maxParticles": 64, "parameters": {}, "emitters": [base]})
}

#[test]
fn documents_normalize_defaults_and_sort_bursts() {
    let compiled = compile_particle_effect(&document(json!({"duration": 2,
        "bursts": [{"time": 1, "count": 1}, {"time": 0.5, "count": 3}]})))
    .unwrap();
    let emitter = &compiled.asset().emitters[0];
    assert_eq!(emitter.bursts.iter().map(|burst| burst.time).collect::<Vec<_>>(), [0.5, 1.0]);
    assert_eq!(compiled.asset().to_json()["emitters"][0]["bursts"][0], json!({"time": 0.5, "count": 3}));
}

#[test]
fn invalid_documents_report_reference_messages() {
    let message = |value: Value| compile_particle_effect(&value).unwrap_err().to_string();
    assert_eq!(
        message(document(json!({"lifetime": 0}))),
        "main.lifetime must be a number from 0.0016666666666666668 to 30."
    );
    assert_eq!(message(document(json!({"renderer": "flipbook"}))), "main: flipbook requires a texture.");
    assert_eq!(
        message(document(json!({"death": {"emitter": "missing", "count": 1}}))),
        "Emitter 'main' references missing child 'missing'."
    );
    assert_eq!(
        message(document(json!({"spawn": "x = missing;"}))),
        "Emitter 'main': Unknown numeric value 'missing' or field is not writable (line 1, column 5)"
    );
}
