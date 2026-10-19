// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/rendering/preview_lighting.rs`.

use super::*;

#[test]
fn settings_validate_their_ranges() {
    assert!(DEFAULT_PREVIEW_LIGHTING.validate().is_ok());
    let error = DEFAULT_PREVIEW_LIGHTING.with(LightingKey::Intensity, 13.0).validate().unwrap_err();
    assert_eq!(error, "Direct intensity must be between 0 and 12.");
    let dark = DEFAULT_PREVIEW_LIGHTING
        .with(LightingKey::DirectionX, 0.0)
        .with(LightingKey::DirectionY, 0.0)
        .with(LightingKey::DirectionZ, 0.0);
    assert_eq!(dark.validate().unwrap_err(), "Light direction must be nonzero.");
    assert!(DEFAULT_PREVIEW_LIGHTING.with(LightingKey::ColorR, f64::NAN).validate().is_err());
}

#[test]
fn the_studio_has_two_bright_panels_over_a_gradient() {
    let pixels = create_studio_pixels();
    assert_eq!(pixels.len(), 256 * 128 * 4);
    let at = |x: usize, y: usize| pixels[((y * 256) + x) * 4];
    let panel = at((0.12 * 256.0) as usize, (0.38 * 128.0) as usize);
    let wall = at(64, (0.38 * 128.0) as usize);
    assert!(panel > 5.0 && wall < 0.5, "panel {panel}, wall {wall}");
    assert!(at(64, 0) > at(64, 127), "the ceiling is brighter than the floor");
}
