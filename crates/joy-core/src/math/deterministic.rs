// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Transcendental functions that return identical results on every target.
//!
//! The standard library's `f64::sin`, `f64::atan2`, `f64::exp`, and friends call the platform's C library natively and
//! a different implementation in WebAssembly, so the same seeded simulation could drift between a native build and a
//! browser build. These functions route through the portable `libm` crate instead. Each mirrors the `f64` method of the
//! same name (`deterministic::atan2(y, x)` is `y.atan2(x)`), so call sites read the same.
//!
//! Arithmetic, `sqrt`, `abs`, `floor`, `round`, `min`, and `max` are exact or IEEE correctly rounded and already
//! identical everywhere, as are vector `length`, `distance`, and `normalize`; only the functions here need a portable
//! implementation. Simulation crates and games call these instead of the standard methods, and clippy enforces it.

/// Sine of `value` radians.
pub fn sin(value: f64) -> f64 {
    libm::sin(value)
}

/// Cosine of `value` radians.
pub fn cos(value: f64) -> f64 {
    libm::cos(value)
}

/// Tangent of `value` radians.
pub fn tan(value: f64) -> f64 {
    libm::tan(value)
}

/// Arcsine in radians within `[-π/2, π/2]`; NaN outside `[-1, 1]`.
pub fn asin(value: f64) -> f64 {
    libm::asin(value)
}

/// Arccosine in radians within `[0, π]`; NaN outside `[-1, 1]`.
pub fn acos(value: f64) -> f64 {
    libm::acos(value)
}

/// Arctangent in radians within `[-π/2, π/2]`.
pub fn atan(value: f64) -> f64 {
    libm::atan(value)
}

/// The angle of `(x, y)` from +X in radians within `[-π, π]` (`y.atan2(x)`).
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// `e` raised to `value`.
pub fn exp(value: f64) -> f64 {
    libm::exp(value)
}

/// Natural logarithm.
pub fn ln(value: f64) -> f64 {
    libm::log(value)
}

/// `base` raised to a floating-point `exponent` (`base.powf(exponent)`).
pub fn powf(base: f64, exponent: f64) -> f64 {
    libm::pow(base, exponent)
}

/// Length of the vector `(x, y)` without intermediate overflow or underflow (`x.hypot(y)`).
pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

/// Sine and cosine of `value` radians together (`value.sin_cos()`).
pub fn sin_cos(value: f64) -> (f64, f64) {
    libm::sincos(value)
}

/// Hyperbolic sine.
pub fn sinh(value: f64) -> f64 {
    libm::sinh(value)
}

/// Hyperbolic cosine.
pub fn cosh(value: f64) -> f64 {
    libm::cosh(value)
}

/// Hyperbolic tangent.
pub fn tanh(value: f64) -> f64 {
    libm::tanh(value)
}

/// Inverse hyperbolic sine.
pub fn asinh(value: f64) -> f64 {
    libm::asinh(value)
}

/// Inverse hyperbolic cosine; NaN below 1.
pub fn acosh(value: f64) -> f64 {
    libm::acosh(value)
}

/// Inverse hyperbolic tangent; NaN outside `[-1, 1]`.
pub fn atanh(value: f64) -> f64 {
    libm::atanh(value)
}

/// 2 raised to `value`.
pub fn exp2(value: f64) -> f64 {
    libm::exp2(value)
}

/// `e^value - 1`, accurate for values near zero.
pub fn exp_m1(value: f64) -> f64 {
    libm::expm1(value)
}

/// Logarithm of `value` in an arbitrary `base` (`value.log(base)`).
pub fn log(value: f64, base: f64) -> f64 {
    libm::log(value) / libm::log(base)
}

/// Base-2 logarithm.
pub fn log2(value: f64) -> f64 {
    libm::log2(value)
}

/// Base-10 logarithm.
pub fn log10(value: f64) -> f64 {
    libm::log10(value)
}

/// `ln(1 + value)`, accurate for values near zero.
pub fn ln_1p(value: f64) -> f64 {
    libm::log1p(value)
}

/// `base` raised to an integer `exponent` (`base.powi(exponent)`).
///
/// Square-and-multiply in a fixed order, the same algorithm the standard library's `powi` runs, using only IEEE
/// multiplication and one division, so `powi(x, 2)` is exactly `x * x` on every target.
pub fn powi(base: f64, exponent: i32) -> f64 {
    let mut remaining = exponent.unsigned_abs();
    let mut factor = base;
    let mut result = 1.0;
    while remaining != 0 {
        if (remaining & 1) != 0 {
            result *= factor;
        }
        remaining >>= 1;
        factor *= factor;
    }
    if exponent < 0 { 1.0 / result } else { result }
}

/// Cube root.
pub fn cbrt(value: f64) -> f64 {
    libm::cbrt(value)
}

#[cfg(test)]
#[path = "../../tests/unit/math/deterministic.rs"]
mod tests;
