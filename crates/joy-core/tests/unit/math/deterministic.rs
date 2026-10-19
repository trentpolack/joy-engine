// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/math/deterministic.rs`.

use super::*;

#[test]
fn results_are_pinned_across_targets() {
    // These exact values are what every target computes; a backend change that alters any of them breaks the
    // replay of seeded simulations recorded on another target.
    assert_eq!(atan2(-3.0, 4.0), -0.6435011087932844);
    assert_eq!(exp(0.7), 2.0137527074704766);
    assert_eq!(tan(0.7), 0.8422883804630794);
    assert_eq!(acos(0.3), 1.2661036727794992);
    assert_eq!(asin(-0.42), -0.43344532006988595);
    assert_eq!(hypot(3.0, 4.0), 5.0);
    assert!(acos(1.5).is_nan() && asin(-1.5).is_nan());
    assert_eq!(powi(1.5, 2), 1.5 * 1.5);
    assert_eq!(powi(2.0, -3), 0.125);
}
