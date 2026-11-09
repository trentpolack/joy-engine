// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn ties_keep_the_first_candidate_and_bounds_reject_unworthy_ones() {
    // Replayed decisions depend on equal scores resolving to enumeration order.
    let mut choice = BestChoice::highest();
    for (candidate, score) in [("hold", 1.0), ("attack", 3.0), ("seize", 3.0)] {
        choice.offer(candidate, score);
    }
    assert_eq!(choice.into_best(), Some("attack"));

    // A bound acts like a range limit; the acceptance test only runs for contenders.
    let mut checks = 0;
    let mut nearest = BestChoice::lowest_below(10.0);
    for (candidate, distance) in [(0, 12.0), (1, 6.0), (2, 8.0), (3, 4.0)] {
        nearest.offer_if(candidate, distance, || {
            checks += 1;
            candidate != 3
        });
    }
    assert_eq!(nearest.best(), Some(&1));
    assert_eq!(checks, 2, "only the improving candidates are checked");
}
