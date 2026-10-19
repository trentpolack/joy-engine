// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/renderer.rs`.

use super::*;

#[test]
fn frustum_culling_rejects_geometry_behind_the_camera() {
    let bounds = Bounds { min: Vec3::splat(-0.5), max: Vec3::splat(0.5) };
    let view = crate::camera::look_at(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y);
    let projection = crate::camera::perspective(1.0, 1.0, 0.1, 100.0);
    let view_projection = projection * view;
    assert!(!outside_frustum(bounds, Mat4::IDENTITY, view_projection));
    assert!(outside_frustum(bounds, Mat4::from_translation(Vec3::new(0.0, 0.0, 20.0)), view_projection));
    assert!(outside_frustum(bounds, Mat4::from_translation(Vec3::new(50.0, 0.0, 0.0)), view_projection));
}
