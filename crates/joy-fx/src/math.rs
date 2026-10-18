// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! JavaScript-compatible numeric helpers for particle parity.
//!
//! The TypeScript particle runtime runs on JavaScript numbers (`f64`) and `Math.*`. These helpers reproduce the
//! JavaScript semantics that differ from Rust's standard library (`Math.round`, `Math.min`/`Math.max` with NaN and
//! signed zeros, `**` special cases, V8's `Math.hypot` summation) and route transcendental functions through the `libm`
//! crate, so results are identical natively and in WebAssembly. `exp`, `tan`, and `atan2` match V8 bit for bit; V8's
//! `sin`, `cos`, `log`, and `pow` differ from FreeBSD's in the last bit for roughly one to four percent of inputs (see
//! `docs/rust-particles.md`).

/// `Math.sin`.
pub fn sin(value: f64) -> f64 {
    libm::sin(value)
}

/// `Math.cos`.
pub fn cos(value: f64) -> f64 {
    libm::cos(value)
}

/// `Math.tan`.
pub fn tan(value: f64) -> f64 {
    libm::tan(value)
}

/// `Math.exp`.
pub fn exp(value: f64) -> f64 {
    libm::exp(value)
}

/// `Math.log` (natural logarithm).
pub fn log(value: f64) -> f64 {
    libm::log(value)
}

/// `Math.atan2(y, x)`.
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// `Math.sqrt`; IEEE square root is correctly rounded everywhere.
pub fn sqrt(value: f64) -> f64 {
    value.sqrt()
}

/// `Math.pow` and the `**` operator, including ECMAScript's NaN results where C's `pow` returns one.
pub fn pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() || (base.abs() == 1.0 && exponent.is_infinite()) {
        return f64::NAN;
    }
    libm::pow(base, exponent)
}

/// `Math.round`: nearest integer with ties toward positive infinity, preserving negative zero.
pub fn round(value: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let floor = value.floor();
    let rounded = if value - floor >= 0.5 { floor + 1.0 } else { floor };
    // Values in [-0.5, 0) round to negative zero in JavaScript.
    if rounded == 0.0 && value < 0.0 { -0.0 } else { rounded }
}

/// `Math.min` for two arguments: NaN propagates and negative zero is smaller than positive zero.
pub fn min(left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        return f64::NAN;
    }
    if left < right {
        return left;
    }
    if right < left {
        return right;
    }
    if left.is_sign_negative() { left } else { right }
}

/// `Math.max` for two arguments: NaN propagates and positive zero is larger than negative zero.
pub fn max(left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        return f64::NAN;
    }
    if left > right {
        return left;
    }
    if right > left {
        return right;
    }
    if left.is_sign_positive() { left } else { right }
}

/// `Math.hypot(a, b, c)` exactly as V8 computes it: values scaled by the largest magnitude and Kahan-summed.
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    let values = [a, b, c];
    let mut largest = 0.0_f64;
    let mut any_nan = false;
    for value in values {
        if value.is_nan() {
            any_nan = true;
        } else if value.abs() > largest {
            largest = value.abs();
        }
    }
    if largest == f64::INFINITY {
        return f64::INFINITY;
    }
    if any_nan {
        return f64::NAN;
    }
    if largest == 0.0 {
        return 0.0;
    }

    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in values {
        let normalized = value.abs() / largest;
        let summand = (normalized * normalized) - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * largest
}

/// JavaScript truthiness of a number: zero and NaN are false.
pub fn truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

/// `Number.isInteger`.
pub fn is_integer(value: f64) -> bool {
    value.is_finite() && value.trunc() == value
}

/// ECMAScript `ToUint32` of a finite integer (`value >>> 0`), exact for every magnitude.
pub fn to_uint32(value: f64) -> u32 {
    const MODULUS: f64 = 4_294_967_296.0;
    let remainder = value.trunc() % MODULUS;
    let positive = if remainder < 0.0 { remainder + MODULUS } else { remainder };
    positive as u32
}

/// Format a number the way JavaScript's `String(number)` does for messages (`0.016666666666666666`, `1e-7`, `0`).
pub fn number_text(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    joy_core::format::format_bound(value)
}

/// JavaScript whitespace (`\s` and `String.prototype.trim`): Unicode spaces, line terminators, and the BOM.
pub fn is_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// Length in UTF-16 code units, the unit of JavaScript's `string.length`.
pub fn utf16_length(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_and_extrema_follow_javascript() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert!(round(-0.4).is_sign_negative());
        assert_eq!(round(0.49999999999999994), 0.0);
        assert!(min(0.0, -0.0).is_sign_negative());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert!(min(f64::NAN, 1.0).is_nan());
        assert!(pow(1.0, f64::NAN).is_nan());
        assert!(pow(-1.0, f64::INFINITY).is_nan());
        assert_eq!(pow(f64::NAN, 0.0), 1.0);
    }

    #[test]
    fn helpers_match_recorded_v8_values() {
        // Recorded with Node 22: Math.hypot(3, 4, 12), Math.exp(0.7), (-7) >>> 0, (2 ** 40 + 5) >>> 0.
        assert_eq!(hypot3(3.0, 4.0, 12.0), 13.0);
        assert_eq!(exp(0.7), 2.0137527074704766);
        assert_eq!(to_uint32(-7.0), 4_294_967_289);
        assert_eq!(to_uint32(1_099_511_627_781.0), 5);
        assert_eq!(number_text(1.0 / 600.0), "0.0016666666666666668");
        assert_eq!(number_text(-1e6), "-1000000");
        assert_eq!(number_text(1e-7), "1e-7");
    }
}
