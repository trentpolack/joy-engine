// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use std::f64::consts::FRAC_PI_2;

use super::*;

#[test]
fn steering_is_turn_limited_and_seekers_lose_targets_outside_the_cone() {
    // A pursuer heading +X toward a target straight above turns by the limit and keeps its speed.
    let velocity = steer_velocity_toward(DVec2::new(2.0, 0.0), DVec2::ZERO, DVec2::new(0.0, 10.0), 0.1);
    assert!((velocity.length() - 2.0).abs() < 1e-12);
    assert!((deterministic::atan2(velocity.y, velocity.x) - 0.1).abs() < 1e-12);

    // A seeker turns toward a target inside its cone and ignores one outside it.
    let heading = DVec3::Z;
    let inside = DVec3::new(0.2, 0.0, 1.0).normalize();
    let turned = home_direction(heading, inside, 0.5, 0.05);
    assert!(turned.x > 0.0 && (turned.length() - 1.0).abs() < 1e-12);
    assert_eq!(home_direction(heading, DVec3::X, FRAC_PI_2 - 0.1, 0.05), heading);
}
