// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;
use serde_json::json;

#[test]
fn keys_follow_every_evaluation_input_but_not_parameter_order() {
    let sources = SourceFingerprint::new("output(\"Box\", box(1, 1, 1))", []);
    let key = evaluation_key(sources, &json!({"width": 2, "height": 3}), None);

    // Parameter order is not an input: the same overrides in another order find the same cooked copy.
    assert_eq!(key, evaluation_key(sources, &json!({"height": 3, "width": 2}), None));

    // A new evaluator build, an edited module, a changed parameter, and a palette each miss the old copy.
    let module = SourceFingerprint::new("output(\"Box\", box(1, 1, 1))", [("kit.form", "fn a() { return 1 }")]);
    let misses = [
        key_for_version("0.0.0+other", sources, &json!({"width": 2, "height": 3}), None),
        evaluation_key(module, &json!({"width": 2, "height": 3}), None),
        evaluation_key(sources, &json!({"width": 2.5, "height": 3}), None),
        evaluation_key(sources, &json!({"width": 2, "height": 3}), Some(&json!([]))),
    ];
    for miss in misses {
        assert_ne!(miss, key);
    }
    assert_eq!(CookKey::from_bytes(key.to_bytes()), key);
    assert_eq!(key.to_hex().len(), 32);
}
