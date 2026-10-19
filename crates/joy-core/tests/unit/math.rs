// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/math/mod.rs`.

use super::*;

#[test]
fn rectangle_edge_distance_selects_nearest_side() {
    assert!((distance_to_rectangle_edge(4.0, 2.0, 0.0) - 2.0).abs() < 1e-12);
    assert!((distance_to_rectangle_edge(4.0, 2.0, std::f64::consts::FRAC_PI_2) - 1.0).abs() < 1e-12);
}
