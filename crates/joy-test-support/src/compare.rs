// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Structural JSON comparison with a numeric tolerance.
//!
//! Numbers agree when they differ by at most the tolerance times the larger of one and the recorded magnitude, so a
//! tolerance is relative for large values and absolute near zero. Everything else (strings, booleans, nulls, array
//! lengths, and object keys) must match exactly; object key order does not matter.

use serde_json::Value;

/// Most mismatches one comparison collects; a broken recording reports its first differences, not all of them.
const MAX_MISMATCHES: usize = 200;

/// How closely recorded numbers must agree.
#[derive(Clone, Debug, PartialEq)]
pub struct Tolerance {
    relative: f64,
    // Object keys whose subtrees use their own tolerance, in the order they were added.
    keys: Vec<(String, f64)>,
}

impl Tolerance {
    /// Numbers must be equal as `f64` values (`2` and `2.0` are equal).
    pub fn exact() -> Self {
        Self::relative(0.0)
    }

    /// Numbers may differ by `relative` times the larger of one and the recorded magnitude.
    pub fn relative(relative: f64) -> Self {
        Self { relative, keys: Vec::new() }
    }

    /// Use `relative` for every number beneath an object key named `key`, at any depth, such as a looser tolerance
    /// for sums that accumulate many single-precision steps.
    pub fn with_key(mut self, key: &str, relative: f64) -> Self {
        self.keys.push((key.to_owned(), relative));
        self
    }

    /// The tolerance at the root of a comparison.
    pub(crate) fn root(&self) -> f64 {
        self.relative
    }

    /// The tolerance for the subtree under object key `key`, given its parent's.
    pub(crate) fn for_key(&self, key: &str, inherited: f64) -> f64 {
        self.keys.iter().find(|(name, _)| name == key).map_or(inherited, |(_, relative)| *relative)
    }

    /// Whether two numbers agree within `relative`.
    pub(crate) fn agrees(relative: f64, actual: f64, recorded: f64) -> bool {
        actual == recorded || (actual - recorded).abs() <= relative * recorded.abs().max(1.0)
    }
}

/// One difference between produced and recorded JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    /// JSON path from the compared root, such as `$.cases[2].frame.sun`.
    pub path: String,
    /// What differs.
    pub detail: String,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.path, self.detail)
    }
}

/// Compare `actual` (what the code produces now) with `recorded`; returns the differences, at most a few hundred.
pub fn compare(actual: &Value, recorded: &Value, tolerance: &Tolerance) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();
    compare_into(actual, recorded, "$", tolerance, tolerance.root(), &mut mismatches);
    mismatches
}

fn compare_into(
    actual: &Value,
    recorded: &Value,
    path: &str,
    tolerance: &Tolerance,
    relative: f64,
    mismatches: &mut Vec<Mismatch>,
) {
    if mismatches.len() >= MAX_MISMATCHES {
        return;
    }
    let mut differ = |detail: String| mismatches.push(Mismatch { path: path.to_owned(), detail });

    match (actual, recorded) {
        (Value::Number(actual), Value::Number(recorded)) => {
            let (actual, recorded) = (number(actual), number(recorded));
            if !Tolerance::agrees(relative, actual, recorded) {
                differ(format!("{actual} != recorded {recorded}"));
            }
        }
        (Value::Array(actual), Value::Array(recorded)) => {
            if actual.len() != recorded.len() {
                differ(format!("length {} != recorded {}", actual.len(), recorded.len()));
                return;
            }
            for (index, (actual, recorded)) in actual.iter().zip(recorded).enumerate() {
                compare_into(actual, recorded, &format!("{path}[{index}]"), tolerance, relative, mismatches);
            }
        }
        (Value::Object(actual), Value::Object(recorded)) => {
            for key in recorded.keys().filter(|key| !actual.contains_key(*key)) {
                mismatches.push(Mismatch { path: format!("{path}.{key}"), detail: "recorded but not produced".into() });
            }
            for (key, actual) in actual {
                let child = format!("{path}.{key}");
                match recorded.get(key) {
                    Some(recorded) => {
                        let relative = tolerance.for_key(key, relative);
                        compare_into(actual, recorded, &child, tolerance, relative, mismatches);
                    }
                    None => mismatches.push(Mismatch { path: child, detail: "produced but not recorded".into() }),
                }
            }
        }
        (actual, recorded) if actual == recorded => {}
        (actual, recorded) => differ(format!("{actual} != recorded {recorded}")),
    }
}

/// A JSON number as `f64`; every number serde_json parses or writes has one.
pub(crate) fn number(value: &serde_json::Number) -> f64 {
    value.as_f64().unwrap_or(f64::NAN)
}

#[cfg(test)]
#[path = "compare.test.rs"]
mod tests;
