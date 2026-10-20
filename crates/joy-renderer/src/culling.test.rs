// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/culling.rs`.

use super::*;
use crate::camera::{
    look_at,
    perspective,
};

#[test]
fn spheres_and_bounds_are_culled_only_when_wholly_outside_a_view() {
    // A 90-degree camera at the origin looking down -Z, seeing 1 to 50 units.
    let view_projection =
        perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 50.0) * look_at(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);

    // The side planes lean at 45 degrees, so a point at (10, 0, -5) is 5/√2 ≈ 3.54 units beyond the right plane.
    assert!(!sphere_outside_view(Vec3::new(0.0, 0.0, -10.0), 1.0, view_projection));
    assert!(sphere_outside_view(Vec3::new(10.0, 0.0, -5.0), 3.0, view_projection));
    assert!(!sphere_outside_view(Vec3::new(10.0, 0.0, -5.0), 4.0, view_projection), "reaches into the view");
    assert!(sphere_outside_view(Vec3::new(0.0, 0.0, 5.0), 3.0, view_projection), "behind the near plane");
    assert!(!sphere_outside_view(Vec3::new(0.0, 0.0, -52.0), 3.0, view_projection), "reaches back past the far plane");

    // Boxes straddling a plane stay; boxes beyond the far plane go.
    let straddling = Bounds {
        min: Vec3::new(-100.0, -1.0, -20.0),
        max: Vec3::new(100.0, 1.0, -10.0),
    };
    let beyond = Bounds {
        min: Vec3::new(-1.0, -1.0, -80.0),
        max: Vec3::new(1.0, 1.0, -60.0),
    };
    assert!(!bounds_outside_view(straddling, view_projection));
    assert!(bounds_outside_view(beyond, view_projection));
    assert!(sphere_touches_bounds(Vec3::new(0.0, 3.0, -15.0), 2.0, straddling));
    assert!(!sphere_touches_bounds(Vec3::new(0.0, 3.5, -15.0), 2.0, straddling));
}

#[test]
fn frustum_culling_rejects_geometry_behind_the_camera() {
    let bounds = Bounds {
        min: Vec3::splat(-0.5),
        max: Vec3::splat(0.5),
    };
    let view_projection = perspective(1.0, 1.0, 0.1, 100.0) * look_at(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y);
    assert!(!outside_frustum(bounds, Mat4::IDENTITY, view_projection));
    assert!(outside_frustum(bounds, Mat4::from_translation(Vec3::new(0.0, 0.0, 20.0)), view_projection));
    assert!(outside_frustum(bounds, Mat4::from_translation(Vec3::new(50.0, 0.0, 0.0)), view_projection));
}
