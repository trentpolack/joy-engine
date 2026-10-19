// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/character.rs`.

use super::*;
use serde_json::json;

#[test]
fn intent_records_round_trip_and_reject_non_actions() {
    let intent = ControlIntent::from_json(&json!({"throttle": 1, "brake": false, "steer": -0.5})).unwrap();
    assert_eq!(intent, ControlIntent::new().with("throttle", 1.0).with("brake", false).with("steer", -0.5));
    assert_eq!(intent.to_json().to_string(), r#"{"throttle":1,"brake":false,"steer":-0.5}"#);
    assert_eq!(ControlIntent::from_json(&json!([1])), Err(GameplayError::InvalidIntentRecord));
    assert_eq!(ControlIntent::from_json(&json!({"throttle": "full"})), Err(GameplayError::NonFiniteIntent));
}
