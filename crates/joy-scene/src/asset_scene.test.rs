// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/asset_scene.rs`.

use super::*;

#[test]
fn rotation_inverts() {
    let angles = [0.3, -1.1, 2.0];
    let point = [1.0, 2.0, 3.0];
    let back = rotate(rotate(point, angles, false), angles, true);
    for axis in 0..3 {
        assert!((back[axis] - point[axis]).abs() < 1e-12);
    }
}
