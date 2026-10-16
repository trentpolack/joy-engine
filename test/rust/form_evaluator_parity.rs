// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Evaluator edge cases recorded from the TypeScript reference.

use joy_engine::form::compile;
use serde_json::json;

#[test]
fn rounding_preserves_negative_zero_for_downstream_signed_math() {
    let result =
        compile("point(atan2(round(-0),-1),atan2(round(-0.1),-1),atan2(round(0),-1))", &json!({}), None).unwrap();
    assert_eq!(result.positions, vec![-std::f64::consts::PI, -std::f64::consts::PI, std::f64::consts::PI]);
}

#[test]
fn scalar_override_primitives_keep_typescript_diagnostics_and_location() {
    for supplied in [json!("3"), json!(true)] {
        let error =
            compile("\nparam amount=2,0,4,1 point(amount,0,0)", &json!({"amount": supplied}), None).unwrap_err();
        assert_eq!(error.message, "Expected a finite scalar; parameter metadata requires min, max, and step.");
        assert_eq!((error.line, error.column), (2, 1));
    }
}
