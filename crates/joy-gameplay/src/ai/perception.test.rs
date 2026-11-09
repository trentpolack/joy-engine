// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn nearest_respects_range_and_cones_ignore_targets_behind() {
    let candidates = [("far", DVec2::new(9.0, 0.0)), ("near", DVec2::new(0.0, 3.0)), ("tied", DVec2::new(3.0, 0.0))];
    let found = nearest(DVec2::ZERO, candidates, 10.0).expect("a candidate is in range");
    assert_eq!((found.candidate, found.distance), ("near", 3.0));
    assert!(nearest(DVec2::ZERO, candidates, 3.0).is_none(), "the range is exclusive");

    let cone = SightCone {
        origin: DVec3::ZERO,
        direction: DVec3::Z,
    };
    assert!(cone.sight(DVec3::new(0.0, 0.0, -5.0)).is_none());
    let ahead = cone.sight(DVec3::new(1.0, 0.0, 1.0)).expect("ahead of the origin");
    assert!((ahead.angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
}
