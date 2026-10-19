// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/random.rs`.

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
