// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/volumetrics.rs`.

use super::*;
use crate::scene_effects::config::VolumetricQuality;

#[test]
fn froxel_grid_stays_bounded_on_large_targets() {
    // A 4K target widens the cells instead of growing the volume past the cap.
    let high = VolumetricOptions {
        quality: VolumetricQuality::High,
        ..VolumetricOptions::default()
    };
    let [columns, rows, slices] = froxel_grid((3840, 2160), &high);
    assert!(columns <= MAX_GRID_EXTENT && rows <= MAX_GRID_EXTENT);
    assert_eq!(slices, 96);

    // Small targets keep the quality's cell size, and every axis covers the whole target.
    let [columns, rows, _] = froxel_grid((640, 360), &high);
    assert_eq!((columns, rows), (80, 45));
    assert_eq!(froxel_grid((1, 1), &high)[..2], [1, 1]);
}
