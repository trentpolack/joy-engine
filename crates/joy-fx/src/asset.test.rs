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

    // The default blend is straight alpha and stays out of saved documents; other modes round trip.
    assert_eq!(emitter.blend, ParticleBlend::Alpha);
    assert!(compiled.asset().to_json()["emitters"][0].get(PARTICLE_EMITTER_BLEND).is_none());
    let additive = compile_particle_effect(&document(json!({"blend": "additive"}))).unwrap();
    assert_eq!(additive.asset().emitters[0].blend, ParticleBlend::Additive);
    let saved = additive.asset().to_json();
    assert_eq!(saved["emitters"][0][PARTICLE_EMITTER_BLEND], json!("additive"));
    assert_eq!(compile_particle_effect(&saved).unwrap().asset(), additive.asset());

    // Soft and lit particles follow the same rule: hard, unlit emitters save exactly as before, and the authored
    // distance and mode round trip.
    assert_eq!((emitter.softness, emitter.lighting), (0.0, ParticleLighting::Unlit));
    let saved = compiled.asset().to_json();
    assert!(saved["emitters"][0].get(PARTICLE_EMITTER_SOFTNESS).is_none());
    assert!(saved["emitters"][0].get(PARTICLE_EMITTER_LIGHTING).is_none());
    let soft = compile_particle_effect(&document(json!({"softness": 1.5, "lighting": "lit"}))).unwrap();
    assert_eq!((soft.asset().emitters[0].softness, soft.asset().emitters[0].lighting), (1.5, ParticleLighting::Lit));
    let saved = soft.asset().to_json();
    assert_eq!(saved["emitters"][0][PARTICLE_EMITTER_SOFTNESS], json!(1.5));
    assert_eq!(saved["emitters"][0][PARTICLE_EMITTER_LIGHTING], json!("lit"));
    assert_eq!(compile_particle_effect(&saved).unwrap().asset(), soft.asset());
}

#[test]
fn invalid_documents_report_reference_messages() {
    let message = |value: Value| compile_particle_effect(&value).unwrap_err().to_string();
    assert_eq!(
        message(document(json!({"lifetime": 0}))),
        "main.lifetime must be a number from 0.0016666666666666668 to 30."
    );
    assert_eq!(message(document(json!({"renderer": "flipbook"}))), "main: flipbook requires a texture.");
    assert_eq!(message(document(json!({"blend": "screen"}))), "main: unknown particle blend mode.");
    assert_eq!(message(document(json!({"softness": -1}))), "main.softness must be a number from 0 to 10000.");
    assert_eq!(message(document(json!({"lighting": "phong"}))), "main: unknown particle lighting mode.");
    assert_eq!(
        message(document(json!({"death": {"emitter": "missing", "count": 1}}))),
        "Emitter 'main' references missing child 'missing'."
    );
    assert_eq!(
        message(document(json!({"spawn": "x = missing;"}))),
        "Emitter 'main': Unknown numeric value 'missing' or field is not writable (line 1, column 5)"
    );
}
