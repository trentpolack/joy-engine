// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/key_repeat.rs`.

use super::*;

#[test]
fn held_keys_fire_once_then_repeat_after_the_delay() {
    let mut repeat = KeyRepeat::default();
    assert_eq!(repeat.update(true, 0.016), 1);
    let early: u32 = (0..20).map(|_| repeat.update(true, 0.02)).sum();
    assert_eq!(early, 0);
    let later: u32 = (0..30).map(|_| repeat.update(true, 0.02)).sum();
    assert!((15..=20).contains(&later), "{later}");
    assert_eq!(repeat.update(false, 0.02), 0);
    assert_eq!(repeat.update(true, 0.02), 1);
}
