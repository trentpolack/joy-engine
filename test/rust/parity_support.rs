// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared helpers for comparing Rust output with JSON recorded from the TypeScript engine.

use serde::Serialize;
use serde_json::Value;

/// Assert that a serializable Rust value equals recorded TypeScript JSON.
///
/// Numbers compare as exact `f64` values (JavaScript writes `2`, serde may write `2.0`), object keys compare without
/// regard to order, and the first mismatch is reported with its JSON path.
pub fn assert_matches_recording(actual: &impl Serialize, expected: &Value, context: &str) {
    let actual = serde_json::to_value(actual).expect("test values serialize");
    if let Err(mismatch) = compare(&actual, expected, "$") {
        panic!("{context}: {mismatch}");
    }
}

/// Recursive comparison; returns the first mismatch.
fn compare(actual: &Value, expected: &Value, path: &str) -> Result<(), String> {
    match (actual, expected) {
        (Value::Number(left), Value::Number(right)) => {
            let (left, right) = (left.as_f64(), right.as_f64());
            if left == right {
                return Ok(());
            }
            Err(format!("{path}: {left:?} != {right:?}"))
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                return Err(format!("{path}: length {} != {}", left.len(), right.len()));
            }
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                compare(left, right, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        (Value::Object(left), Value::Object(right)) => {
            if left.len() != right.len() {
                let mut left_keys: Vec<_> = left.keys().collect();
                let mut right_keys: Vec<_> = right.keys().collect();
                left_keys.sort();
                right_keys.sort();
                return Err(format!("{path}: keys {left_keys:?} != {right_keys:?}"));
            }
            for (key, value) in left {
                let Some(other) = right.get(key) else {
                    return Err(format!("{path}: unexpected key {key}"));
                };
                compare(value, other, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        _ if actual == expected => Ok(()),
        _ => Err(format!("{path}: {actual} != {expected}")),
    }
}
