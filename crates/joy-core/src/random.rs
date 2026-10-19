// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Deterministic random streams.
//!
//! `SeededRandom` preserves the retired TypeScript engine's `createSeededRandom` sequence bit for bit. Seeded
//! gameplay and cosmetic sequences match recorded fixtures and agree across native and WebAssembly builds.

/// Linear congruential multiplier shared with the TypeScript engine (Numerical Recipes constants).
const LCG_MULTIPLIER: u32 = 1_664_525;

/// Linear congruential increment shared with the TypeScript engine.
const LCG_INCREMENT: u32 = 1_013_904_223;

/// Divisor that maps a 32-bit state into the half-open unit interval.
const UNIT_DIVISOR: f64 = 4_294_967_296.0;

/// A source of uniformly distributed values in `[0, 1)`.
///
/// Engine helpers take a `RandomSource` instead of reading a global generator, so callers can keep gameplay and
/// cosmetic streams separate.
pub trait RandomSource {
    /// Return the next value in the half-open interval `[0, 1)` and advance the stream.
    fn next_unit(&mut self) -> f64;
}

/// Independently owned deterministic random stream in `[0, 1)`.
///
/// Keep separate instances for gameplay and presentation so changing cosmetic draw counts cannot alter simulation
/// decisions. This generator is not cryptographic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeededRandom {
    // Only the 32-bit sequence state is retained; there is no shared or global generator.
    state: u32,
}

impl SeededRandom {
    /// Create a stream from an integer seed, normalized to an unsigned 32-bit value exactly like JavaScript's
    /// `seed >>> 0` (negative and oversized seeds wrap modulo 2^32).
    pub fn new(seed: i64) -> Self {
        let normalized = seed.rem_euclid(1_i64 << 32) as u32;
        Self { state: normalized }
    }

    /// Current 32-bit sequence state, useful for saving and restoring a stream.
    pub fn state(&self) -> u32 {
        self.state
    }
}

impl RandomSource for SeededRandom {
    fn next_unit(&mut self) -> f64 {
        // `Math.imul` followed by `>>> 0` is 32-bit wrapping arithmetic.
        self.state = self.state.wrapping_mul(LCG_MULTIPLIER).wrapping_add(LCG_INCREMENT);
        f64::from(self.state) / UNIT_DIVISOR
    }
}

/// Sample a floating-point value from the half-open range `[minimum, maximum)` using an injected stream.
pub fn random_range(minimum: f64, maximum: f64, random: &mut impl RandomSource) -> f64 {
    minimum + (random.next_unit() * (maximum - minimum))
}

/// Return an integer in `[minimum, maximum_exclusive)`; consumes exactly one draw, matching the TypeScript helper.
pub fn random_integer(minimum: i64, maximum_exclusive: i64, random: &mut impl RandomSource) -> i64 {
    random_range(minimum as f64, maximum_exclusive as f64, random).floor() as i64
}

#[cfg(test)]
#[path = "random.test.rs"]
mod tests;
