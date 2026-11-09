// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn sweeps_report_the_earliest_contact() {
    // A fast mover crossing a small circle registers on entry, not after passing through it.
    let entry = segment_circle_entry(DVec2::new(-10.0, 0.0), DVec2::new(10.0, 0.0), DVec2::ZERO, 1.0);
    assert!((entry - 0.45).abs() < 1e-12);
    assert!(segment_circle_entry(DVec2::new(-10.0, 5.0), DVec2::new(10.0, 5.0), DVec2::ZERO, 1.0).is_infinite());

    // A ray starting inside a box enters at zero; one aimed away misses.
    let minimum = DVec3::splat(-1.0);
    let maximum = DVec3::splat(1.0);
    assert_eq!(ray_box_distance(DVec3::ZERO, DVec3::X, minimum, maximum), Some(0.0));
    assert_eq!(ray_box_distance(DVec3::new(5.0, 0.0, 0.0), DVec3::X, minimum, maximum), None);

    // Crossing segments meet; the parameter locates the meeting point on the first.
    let approach = segment_segment_distance(
        DVec3::new(-1.0, 0.0, 0.0),
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.0, -1.0, 0.0),
        DVec3::new(0.0, 1.0, 0.0),
    );
    assert!(approach.distance < 1e-12);
    assert!((approach.first_parameter - 0.5).abs() < 1e-12);
    let closest = point_segment_distance(DVec3::new(0.0, 2.0, 0.0), DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0));
    assert_eq!(closest.distance, 2.0);
}
