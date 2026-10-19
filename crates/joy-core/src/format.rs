// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Number formatting shared by error messages, interface text, and JSON writers.
//!
//! [`format_number`] writes the shortest decimal that round-trips, without a trailing `.0` on integers and with an
//! exponent only for very large or very small magnitudes, so authored documents and messages stay compact and stable.

use serde_json::Value;

/// Largest integer magnitude an `f64` represents exactly together with all its neighbors (2^53 - 1).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Smallest magnitude written without an exponent.
const MIN_FIXED_MAGNITUDE: f64 = 1e-6;

/// Magnitude from which numbers are written with an exponent.
const MIN_EXPONENT_MAGNITUDE: f64 = 1e21;

/// Format a number as the shortest decimal that round-trips (`2`, `0.1`, `1.5e-7`, `1e+21`).
///
/// Magnitudes below `1e-6` and from `1e21` use exponent notation (with an explicit `+` for positive exponents),
/// negative zero is written `0`, and the non-finite values are spelled `NaN`, `Infinity`, and `-Infinity`.
pub fn format_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }

    let magnitude = value.abs();
    if !(MIN_FIXED_MAGNITUDE..MIN_EXPONENT_MAGNITUDE).contains(&magnitude) {
        let text = format!("{value:e}");
        return match text.split_once('e') {
            Some((mantissa, exponent)) if !exponent.starts_with('-') => format!("{mantissa}e+{exponent}"),
            _ => text,
        };
    }
    format!("{value}")
}

/// Convert a number to the JSON value authored documents store.
///
/// Integral values within `±(2^53 - 1)` become JSON integers (`1`, never `1.0`, and `-0` becomes `0`); other finite
/// values keep their shortest round-trip form; non-finite values, which JSON cannot represent, become `null`.
pub fn json_number(value: f64) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER {
        // The range check above keeps the conversion exact.
        return Value::from(value as i64);
    }
    Value::from(value)
}

#[cfg(test)]
#[path = "../tests/unit/format.rs"]
mod tests;
