// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/volume_lights.rs`.

use super::*;
use crate::perspective;

#[test]
fn lights_bin_into_the_tiles_and_slices_they_touch() {
    let projection = perspective(60_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
    let horizontal = tile_planes(projection, 4, 0);
    // A light straight ahead touches the middle tiles but not the outermost ones.
    let (x, y, z, radius) = (0.0, 0.0, -10.0, 0.5);
    assert!(intersects_tile(&horizontal, 1, x, y, z, radius) && intersects_tile(&horizontal, 2, x, y, z, radius));
    assert!(!intersects_tile(&horizontal, 0, x, y, z, radius) && !intersects_tile(&horizontal, 3, x, y, z, radius));
    // A light toward the right edge (NDC x about 0.78) reaches only the right tile; one past the edge reaches none.
    assert!(intersects_tile(&horizontal, 3, 8.0, 0.0, -10.0, 0.5));
    assert!(!intersects_tile(&horizontal, 2, 8.0, 0.0, -10.0, 0.5));
    assert!((0..4).all(|tile| !intersects_tile(&horizontal, tile, 14.0, 0.0, -10.0, 0.5)));
}
