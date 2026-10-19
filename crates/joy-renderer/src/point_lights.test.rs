// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/point_lights.rs`.

use joy_core::LinearColor;
use joy_core::math::Vec3;

use super::*;
use crate::camera::{
    look_at,
    perspective,
};

fn light(x: f32, z: f32, brightness: f32) -> PointLight {
    PointLight {
        position: Vec3::new(x, 1.0, z),
        color: LinearColor::rgb(brightness, brightness, brightness),
        range: 4.0,
        casts_shadow: false,
    }
}

#[test]
fn more_lights_than_the_cap_keep_the_strongest_in_view_instead_of_failing() {
    let eye = Vec3::new(0.0, 2.0, 10.0);
    let camera = CameraFrame {
        view: look_at(eye, Vec3::ZERO, Vec3::Y),
        projection: perspective(1.0, 1.5, 0.1, 100.0),
        eye,
        focus: Vec3::ZERO,
        near: 0.1,
        far: 100.0,
    };

    // A level's worth of dim lights along the view, two bright ones near the camera, one behind the camera (it cannot
    // reach the view), and one with an invalid range.
    let mut lights: Vec<PointLight> =
        (0..40).map(|index| light((index % 8) as f32 - 4.0, -(index as f32), 1.0)).collect();
    lights.push(light(-1.0, 8.0, 50.0));
    lights.push(light(1.0, 8.0, 40.0));
    lights.push(light(0.0, 30.0, 1000.0));
    lights.push(PointLight { range: 0.0, ..light(0.0, 0.0, 10.0) });

    let selected = select_point_lights(&lights, &camera);
    assert_eq!(selected.lights.len(), MAX_POINT_LIGHTS);
    assert_eq!(selected.omitted, lights.len() - MAX_POINT_LIGHTS);
    assert_eq!(selected.lights[0], lights[40], "the brightest nearby light ranks first");
    assert_eq!(selected.lights[1], lights[41]);
    assert!(!selected.lights.contains(&lights[42]), "lights that cannot reach the view are skipped");
    assert!(selected.lights.iter().all(PointLight::is_valid));

    // Among equally bright lights, the ones nearest the camera are kept.
    let farthest_kept = selected.lights.iter().map(|light| light.position.z).fold(f32::INFINITY, f32::min);
    assert!(farthest_kept > -35.0, "{farthest_kept}");
}
