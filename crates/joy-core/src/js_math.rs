// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! JavaScript `Math` semantics for deterministic game simulation.
//!
//! Games ported from the TypeScript engine run their rules on JavaScript numbers (`f64`) and `Math.*`. Rust's standard
//! transcendental functions call the platform C library natively and a different implementation in WebAssembly, so the
//! same seeded simulation could drift between a native build and a browser build. These functions route through the
//! portable `libm` crate (FreeBSD msun ports) instead, so a simulation computes identical results on every target, and
//! reproduce the JavaScript semantics that differ from Rust's (`Math.round` ties, `Math.min`/`Math.max` with NaN and
//! signed zeros, and `**` special cases).
//!
//! Against V8, `exp`, `atan2`, `tan`, `acos`, `asin`, `sqrt`, `floor`, `round`, `min`, and `max` match bit for bit
//! (V8 ports fdlibm for the inverse and tangent functions, as FreeBSD's msun does); V8's `sin`, `cos`, `log`,
//! and `pow` differ from FreeBSD's in the last bit for a few percent of inputs, so parity tests against recorded
//! TypeScript values compare with a small tolerance. Use [`crate::js_hypot`] for `Math.hypot`.

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

/// `Math.acos`, in radians within `[0, π]`; NaN outside `[-1, 1]`.
pub fn acos(value: f64) -> f64 {
    libm::acos(value)
}

/// `Math.asin`, in radians within `[-π/2, π/2]`; NaN outside `[-1, 1]`.
pub fn asin(value: f64) -> f64 {
    libm::asin(value)
}

/// `Math.atan2(y, x)`, the angle of `(x, y)` from +X in radians within `[-π, π]`.
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// `Math.exp`.
pub fn exp(value: f64) -> f64 {
    libm::exp(value)
}

/// `Math.log` (natural logarithm).
pub fn log(value: f64) -> f64 {
    libm::log(value)
}

/// `Math.pow` and the `**` operator, including ECMAScript's NaN results where C's `pow` returns one.
pub fn pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() || (base.abs() == 1.0 && exponent.is_infinite()) {
        return f64::NAN;
    }
    libm::pow(base, exponent)
}

/// `Math.round`: the nearest integer with ties toward positive infinity, preserving negative zero.
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
    fn transcendental_functions_match_recorded_v8_values() {
        // Recorded with Node 22: Math.atan2(-3, 4), Math.exp(0.7), Math.log(0.88), 1.16 ** 3, Math.sin(Math.PI / 5),
        // and Math.cos(2.5).
        assert_eq!(atan2(-3.0, 4.0), -0.6435011087932844);
        assert_eq!(exp(0.7), 2.0137527074704766);
        assert!((log(0.88) - -0.12783337150988489).abs() <= f64::EPSILON);
        assert!((pow(1.16, 3.0) - 1.5608959999999996).abs() <= 2.0 * f64::EPSILON);
        assert!((sin(std::f64::consts::PI / 5.0) - 0.5877852522924731).abs() <= f64::EPSILON);
        assert!((cos(2.5) - -0.8011436155469337).abs() <= f64::EPSILON);
    }

    #[test]
    fn tangent_and_inverse_functions_match_recorded_v8_values_exactly() {
        // Recorded with Node 22: Math.tan(0.7), Math.tan(0.0125), Math.acos(0.3), Math.acos(0.9999), Math.asin(-0.42),
        // and Math.asin(0.5).
        assert_eq!(tan(0.7), 0.8422883804630794);
        assert_eq!(tan(0.0125), 0.012500651082359345);
        assert_eq!(acos(0.3), 1.2661036727794992);
        assert_eq!(acos(0.9999), 0.014142253477512098);
        assert_eq!(asin(-0.42), -0.43344532006988595);
        assert_eq!(asin(0.25), 0.25268025514207865);
        assert!(acos(1.5).is_nan() && asin(-1.5).is_nan());
    }
}
