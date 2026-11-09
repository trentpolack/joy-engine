// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use std::f64::consts::PI;

use super::*;

#[test]
fn wrapping_and_approach_take_the_shorter_arc() {
    // Wrapping keeps in-range values, folds whole turns away, and maps -π to π.
    assert_eq!(wrap_angle(1.0), 1.0);
    assert!((wrap_angle(1.0 + (PI * 4.0)) - 1.0).abs() < 1e-12);
    assert_eq!(wrap_angle(-PI), PI);

    // Approaching across the ±π seam turns the short way instead of sweeping almost a full turn.
    let current = PI - 0.1;
    let target = -PI + 0.1;
    let stepped = approach_angle(current, target, 0.05);
    assert!(stepped > current, "turned the long way: {stepped}");
    assert_eq!(approach_angle(current, target, 0.5), target);

    // The atan2 form agrees on direction and magnitude.
    assert!((angle_difference(current, target) - 0.2).abs() < 1e-12);
}

#[test]
fn damping_is_frame_rate_independent() {
    // Two half steps leave the same remainder as one whole step.
    let whole = 1.0 - damp_factor(6.0, 0.1);
    let half = 1.0 - damp_factor(6.0, 0.05);
    assert!((whole - (half * half)).abs() < 1e-12);
    assert_eq!(damp_factor(6.0, 0.0), 0.0);
}
