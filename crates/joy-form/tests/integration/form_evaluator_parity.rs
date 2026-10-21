// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Evaluator edge cases with recorded expectations: signed zero through rounding, and the diagnostic and location for
//! scalar parameter overrides of the wrong type.

use joy_form::compile;
use serde_json::json;

#[test]
fn rounding_preserves_negative_zero_for_downstream_signed_math() {
    let result = compile(
        "let g=geometry() addPoint(g,vector(atan2(round(-0),-1),atan2(round(-0.1),-1),atan2(round(0),-1))) output(\"P\",g)",
        &json!({}),
        None,
    )
    .unwrap();
    assert_eq!(result.positions, vec![-std::f64::consts::PI, -std::f64::consts::PI, std::f64::consts::PI]);
}

#[test]
fn scalar_override_primitives_keep_stable_diagnostics_and_location() {
    for supplied in [json!("3"), json!(true)] {
        let error = compile("\nparam amount=2,0,4,1", &json!({"amount": supplied}), None).unwrap_err();
        assert_eq!(error.message, "Parameter 'amount' expects a number.");
        assert_eq!((error.line, error.column), (2, 1));
    }
}
