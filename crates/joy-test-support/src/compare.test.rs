// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/compare.rs`.

use serde_json::json;

use super::*;

#[test]
fn numbers_agree_within_a_tolerance_relative_to_the_recorded_magnitude() {
    let recorded = json!({ "small": 0.5, "large": 1000.0, "sums": { "energy": 2.0 } });
    let close = json!({ "small": 0.5 + 5e-10, "large": 1000.0 + 5e-7, "sums": { "energy": 2.0 + 1e-8 } });

    // Near zero the tolerance is absolute; for large values it scales with the magnitude.
    assert!(compare(&close, &recorded, &Tolerance::relative(1e-9).with_key("sums", 1e-7)).is_empty());
    let mismatches = compare(&close, &recorded, &Tolerance::relative(1e-9));
    assert_eq!(mismatches.iter().map(|mismatch| mismatch.path.as_str()).collect::<Vec<_>>(), ["$.sums.energy"]);
    assert!(compare(&json!(2), &json!(2.0), &Tolerance::exact()).is_empty(), "integers equal their float spelling");
}

#[test]
fn structure_strings_and_keys_must_match_exactly() {
    let recorded = json!({ "kind": "sun", "steps": [1, 2], "frame": { "a": 1 } });
    let produced = json!({ "kind": "moon", "steps": [1], "frame": { "b": 1 } });
    let details: Vec<String> =
        compare(&produced, &recorded, &Tolerance::relative(1.0)).iter().map(ToString::to_string).collect();
    assert_eq!(
        details,
        [
            "$.kind: \"moon\" != recorded \"sun\"",
            "$.steps: length 1 != recorded 2",
            "$.frame.a: recorded but not produced",
            "$.frame.b: produced but not recorded",
        ]
    );
}
