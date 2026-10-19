// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/display.rs`.

use super::*;

#[test]
fn density_is_capped_by_ratio_and_pixel_budget() {
    let limits = BackingLimits::default();
    assert_eq!(backing_pixel_ratio(1280.0, 720.0, 1.0, limits), 1.0);
    assert_eq!(backing_pixel_ratio(1280.0, 720.0, 3.0, limits), 2.0);
    // A 4K logical viewport at density 2 exceeds the budget and falls back to 1.
    assert!((backing_pixel_ratio(3840.0, 2160.0, 2.0, limits) - 1.0).abs() < 1e-6);
}
