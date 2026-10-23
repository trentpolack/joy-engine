// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn rerolled_seeds_stay_in_range_change_value_and_repeat_for_the_same_input() {
    let mut seed = 7.0;
    for _ in 0..64 {
        let next = rerolled_seed(seed, 0.0, 100.0);
        assert!((0.0..=100.0).contains(&next) && next.fract() == 0.0);
        assert_ne!(next, seed);
        assert_eq!(rerolled_seed(seed, 0.0, 100.0), next);
        seed = next;
    }
    assert_eq!(rerolled_seed(3.0, 5.0, 5.0), 5.0);
    assert_eq!(rerolled_seed(1.0, 0.0, 1.0), 0.0);
}
