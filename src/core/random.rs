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

#[cfg(test)]
mod tests {
    use super::*;

    /// Values recorded from the TypeScript engine's `createSeededRandom` for the same seeds.
    #[test]
    fn matches_typescript_sequences() {
        let mut stream = SeededRandom::new(1);
        let expected = [0.23645552527159452, 0.3692706737201661, 0.5042420323006809, 0.7048832636792213];
        for value in expected {
            assert_eq!(stream.next_unit(), value);
        }

        let mut negative = SeededRandom::new(-7);
        assert_eq!(negative.next_unit(), 0.23335510585457087);

        let mut oversized = SeededRandom::new(4_294_967_297);
        assert_eq!(oversized.next_unit(), 0.23645552527159452);
    }

    #[test]
    fn independent_streams_do_not_share_state() {
        let mut gameplay = SeededRandom::new(42);
        let mut cosmetic = SeededRandom::new(42);
        let first = gameplay.next_unit();
        cosmetic.next_unit();
        cosmetic.next_unit();
        assert_eq!(SeededRandom::new(42).next_unit(), first);
        assert_ne!(gameplay.state(), cosmetic.state());
    }
}
