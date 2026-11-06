// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The engine's script standard library: exact float comparisons, portable scalar math, the `vec3` type, and the
//! `Random` stream handle.
//!
//! Every function computes exactly what the matching Rust helper computes (IEEE comparisons,
//! `joy_core::math::deterministic`, `clamp`, and glam's `DVec3` operators), so a behavior ported between Rust and script
//! keeps its results bit for bit.

use std::f64::consts::PI;
use std::f64::consts::TAU;

use joy_core::math::DVec3;
use joy_core::math::clamp;
use joy_core::math::deterministic;
use rhai::Engine;
use rhai::INT;

use crate::random::RANDOM_TYPE_NAME;
use crate::random::ScriptRandom;

/// Script type name of `DVec3`.
pub const VECTOR_TYPE_NAME: &str = "vec3";

/// Register the standard library on `engine`.
pub fn register(engine: &mut Engine) {
    register_exact_comparisons(engine);
    register_scalar_math(engine);
    register_vectors(engine);
    register_random(engine);
}

// -------------------------------------------------------------------------------------------------------------------
// Comparisons
// -------------------------------------------------------------------------------------------------------------------

/// IEEE comparisons for floats, and for floats against integers, plus logical not.
///
/// Rhai's built-in float comparisons are approximate in builds with limits enabled: values within a relative
/// `f64::EPSILON` compare equal, so a countdown at `4.9e-17` already passes `timer <= 0.0` a step before the same Rust
/// code does. Ported behavior would drift from its Rust original and from golden recordings. The engine turns off
/// Rhai's operator fast path ([`Engine::set_fast_operators`] in [`crate::ScriptEngine::new`]) so these registered
/// operators replace the built-ins.
fn register_exact_comparisons(engine: &mut Engine) {
    engine
        .register_fn("==", |left: f64, right: f64| left == right)
        .register_fn("!=", |left: f64, right: f64| left != right)
        .register_fn("<", |left: f64, right: f64| left < right)
        .register_fn("<=", |left: f64, right: f64| left <= right)
        .register_fn(">", |left: f64, right: f64| left > right)
        .register_fn(">=", |left: f64, right: f64| left >= right);

    // Mixed operands promote the integer, as Rust code written with `as f64` would.
    engine
        .register_fn("==", |left: INT, right: f64| (left as f64) == right)
        .register_fn("!=", |left: INT, right: f64| (left as f64) != right)
        .register_fn("<", |left: INT, right: f64| (left as f64) < right)
        .register_fn("<=", |left: INT, right: f64| (left as f64) <= right)
        .register_fn(">", |left: INT, right: f64| (left as f64) > right)
        .register_fn(">=", |left: INT, right: f64| (left as f64) >= right)
        .register_fn("==", |left: f64, right: INT| left == (right as f64))
        .register_fn("!=", |left: f64, right: INT| left != (right as f64))
        .register_fn("<", |left: f64, right: INT| left < (right as f64))
        .register_fn("<=", |left: f64, right: INT| left <= (right as f64))
        .register_fn(">", |left: f64, right: INT| left > (right as f64))
        .register_fn(">=", |left: f64, right: INT| left >= (right as f64));

    // Rhai implements `!` only on the fast path; every other operator keeps a registered or built-in fallback.
    engine.register_fn("!", |value: bool| !value);
}

// -------------------------------------------------------------------------------------------------------------------
// Scalars
// -------------------------------------------------------------------------------------------------------------------

/// Portable transcendental functions, exact rounding and comparison helpers, constants, and number conversions.
fn register_scalar_math(engine: &mut Engine) {
    // Transcendental functions differ by platform in the standard library; these use the portable `libm` versions.
    engine
        .register_fn("sin", deterministic::sin)
        .register_fn("cos", deterministic::cos)
        .register_fn("tan", deterministic::tan)
        .register_fn("asin", deterministic::asin)
        .register_fn("acos", deterministic::acos)
        .register_fn("atan", deterministic::atan)
        .register_fn("atan2", deterministic::atan2)
        .register_fn("exp", deterministic::exp)
        .register_fn("ln", deterministic::ln)
        .register_fn("log2", deterministic::log2)
        .register_fn("log10", deterministic::log10)
        .register_fn("powf", deterministic::powf)
        .register_fn("powi", |base: f64, exponent: INT| deterministic::powi(base, saturate_exponent(exponent)))
        .register_fn("hypot", deterministic::hypot);

    // Correctly rounded IEEE operations, identical on every target.
    engine
        .register_fn("sqrt", f64::sqrt)
        .register_fn("floor", f64::floor)
        .register_fn("ceil", f64::ceil)
        .register_fn("round", f64::round)
        .register_fn("trunc", f64::trunc)
        .register_fn("fract", f64::fract)
        .register_fn("min", f64::min)
        .register_fn("max", f64::max)
        .register_fn("min", |left: INT, right: INT| left.min(right))
        .register_fn("max", |left: INT, right: INT| left.max(right))
        .register_fn("clamp", clamp)
        .register_fn("lerp", |from: f64, to: f64, t: f64| from + ((to - from) * t));

    engine
        .register_fn("PI", || PI)
        .register_fn("TAU", || TAU)
        .register_fn("to_float", |value: INT| value as f64)
        .register_fn("to_int", |value: f64| value as INT);
}

/// The script exponent for `powi`, saturated to the range the portable implementation accepts.
fn saturate_exponent(exponent: INT) -> i32 {
    exponent.clamp(INT::from(i32::MIN), INT::from(i32::MAX)) as i32
}

// -------------------------------------------------------------------------------------------------------------------
// Vectors
// -------------------------------------------------------------------------------------------------------------------

/// `vec3(x, y, z)`: Y-up world vectors in `f64`, with components, arithmetic, comparison, and products.
fn register_vectors(engine: &mut Engine) {
    engine
        .register_type_with_name::<DVec3>(VECTOR_TYPE_NAME)
        .register_fn("vec3", DVec3::new)
        .register_get_set("x", |vector: &mut DVec3| vector.x, |vector: &mut DVec3, value: f64| vector.x = value)
        .register_get_set("y", |vector: &mut DVec3| vector.y, |vector: &mut DVec3, value: f64| vector.y = value)
        .register_get_set("z", |vector: &mut DVec3| vector.z, |vector: &mut DVec3, value: f64| vector.z = value);

    // Operators; compound assignment (`+=`) falls back to these.
    engine
        .register_fn("+", |left: DVec3, right: DVec3| left + right)
        .register_fn("-", |left: DVec3, right: DVec3| left - right)
        .register_fn("-", |vector: DVec3| -vector)
        .register_fn("*", |left: DVec3, right: DVec3| left * right)
        .register_fn("*", |vector: DVec3, scale: f64| vector * scale)
        .register_fn("*", |scale: f64, vector: DVec3| scale * vector)
        .register_fn("/", |left: DVec3, right: DVec3| left / right)
        .register_fn("/", |vector: DVec3, divisor: f64| vector / divisor)
        .register_fn("==", |left: &mut DVec3, right: DVec3| *left == right)
        .register_fn("!=", |left: &mut DVec3, right: DVec3| *left != right);

    // Products and measures. `length` is `sqrt(dot)`, exact on every target.
    engine
        .register_fn("length", |vector: &mut DVec3| vector.length())
        .register_fn("length_squared", |vector: &mut DVec3| vector.length_squared())
        .register_fn("distance", |from: &mut DVec3, to: DVec3| from.distance(to))
        .register_fn("dot", |left: &mut DVec3, right: DVec3| left.dot(right))
        .register_fn("cross", |left: &mut DVec3, right: DVec3| left.cross(right))
        .register_fn("lerp", |from: &mut DVec3, to: DVec3, t: f64| from.lerp(to, t))
        .register_fn("to_string", |vector: &mut DVec3| format!("vec3({}, {}, {})", vector.x, vector.y, vector.z))
        .register_fn("to_debug", |vector: &mut DVec3| format!("vec3({}, {}, {})", vector.x, vector.y, vector.z));
}

// -------------------------------------------------------------------------------------------------------------------
// Random Streams
// -------------------------------------------------------------------------------------------------------------------

/// `Random` handles: `random.next_unit()` in `[0, 1)` and `random.range(min, max)`, one draw each, and
/// `seeded_random(seed)` for a script-owned stream.
fn register_random(engine: &mut Engine) {
    engine
        .register_type_with_name::<ScriptRandom>(RANDOM_TYPE_NAME)
        .register_fn("seeded_random", |seed: INT| ScriptRandom::seeded(seed))
        .register_fn("next_unit", |random: &mut ScriptRandom| random.next_unit())
        .register_fn("range", |random: &mut ScriptRandom, minimum: f64, maximum: f64| random.range(minimum, maximum));
}
