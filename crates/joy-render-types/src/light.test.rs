// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/light.rs`.

use super::*;

#[test]
fn skylight_blends_ground_to_sky_by_normal_and_uniform_skylights_match_everywhere() {
    let skylight = Skylight {
        color: LinearColor::rgb(0.2, 0.4, 0.8),
        ground_color: LinearColor::rgb(0.1, 0.1, 0.0),
        intensity: 2.0,
    };
    assert_eq!(skylight.irradiance(Vec3::Y), LinearColor::rgb(0.4, 0.8, 1.6));
    assert_eq!(skylight.irradiance(Vec3::NEG_Y), LinearColor::rgb(0.2, 0.2, 0.0));
    let wall = skylight.irradiance(Vec3::X);
    assert!((wall.red - 0.3).abs() < 1e-6 && (wall.blue - 0.8).abs() < 1e-6, "{wall:?}");

    // A uniform skylight is the flat ambient term every surface received before hemispheres existed.
    let uniform = Skylight::uniform(LinearColor::rgb(0.5, 0.5, 0.5), 1.0);
    for normal in [Vec3::Y, Vec3::NEG_Y, Vec3::X, Vec3::new(0.6, 0.8, 0.0)] {
        assert_eq!(uniform.irradiance(normal), LinearColor::rgb(0.5, 0.5, 0.5));
    }
}
