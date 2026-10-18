// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Number formatting shared by error messages and JSON writers, preserving JavaScript-compatible number text.

use serde_json::Value;

/// Largest magnitude JavaScript represents as an exact integer (`Number.MAX_SAFE_INTEGER`).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Smallest magnitude JavaScript prints without an exponent.
const MIN_FIXED_MAGNITUDE: f64 = 1e-6;

/// Magnitude from which JavaScript prints an exponent.
const MIN_EXPONENT_MAGNITUDE: f64 = 1e21;

/// Format a range bound the way JavaScript prints numbers (`Infinity`, `-Infinity`, `5e-324`, `0.001`).
pub fn format_bound(value: f64) -> String {
    format_js_number(value)
}

/// Format a number exactly like JavaScript's `String(value)`.
///
/// Both languages print the shortest decimal that round trips; JavaScript additionally switches to exponent notation
/// (with an explicit `+` for positive exponents) below `1e-6` and from `1e21`, prints `-0` as `0`, and spells the
/// non-finite values `NaN`, `Infinity`, and `-Infinity`.
pub fn format_js_number(value: f64) -> String {
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

/// Convert a number to the JSON value `JSON.stringify` would write.
///
/// Integral values in JavaScript's safe range become JSON integers (`1`, never `1.0`, and `-0` becomes `0`); other
/// finite values keep their shortest round-trip form; non-finite values become `null`, as in JavaScript.
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
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_javascript() {
        assert_eq!(format_js_number(1.0), "1");
        assert_eq!(format_js_number(-0.0), "0");
        assert_eq!(format_js_number(0.000001), "0.000001");
        assert_eq!(format_js_number(1.5e-7), "1.5e-7");
        assert_eq!(format_js_number(1e21), "1e+21");
        assert_eq!(format_js_number(std::f64::consts::FRAC_PI_2), "1.5707963267948966");
        assert_eq!(format_js_number(f64::NAN), "NaN");
        assert_eq!(json_number(2.0).to_string(), "2");
        assert_eq!(json_number(0.25).to_string(), "0.25");
        assert_eq!(json_number(f64::INFINITY), Value::Null);
    }
}
