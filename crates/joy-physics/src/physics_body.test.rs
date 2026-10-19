// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/physics_body.rs`.

use super::*;
use joy_core::math::deterministic;

#[test]
fn damping_conversion_matches_cannon_decay_over_one_second() {
    // Sixty fixed steps of rapier damping should remove the same velocity fraction as cannon's (1 - d)^1.
    for damping in [0.01_f32, 0.1, 0.5] {
        let coefficient = f64::from(damping_coefficient(damping));
        let dt = 1.0 / 60.0;
        let rapier = deterministic::powi(1.0 / (1.0 + (dt * coefficient)), 60);
        let cannon = deterministic::powf(1.0 - f64::from(damping), 1.0);
        assert!((rapier - cannon).abs() < 0.02 * cannon.max(0.1), "damping {damping}: {rapier} vs {cannon}");
    }
    assert_eq!(damping_coefficient(0.0), 0.0);
    assert_eq!(damping_coefficient(1.0), MAX_DAMPING_COEFFICIENT);
}
