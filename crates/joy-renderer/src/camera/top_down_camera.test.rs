// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/camera/top_down_camera.rs`.

use joy_core::math::{
    Vec3,
    Vec4,
};

use super::*;

#[test]
fn top_down_view_frames_the_viewport_with_world_axes_and_maps_depth() {
    let camera = TopDownCamera {
        height: 100.0,
        near: 1.0,
        far: 201.0,
    };
    let view = TopDownView::new(Vec2::new(800.0, 600.0), Vec2::new(400.0, 300.0), camera).unwrap();
    let matrix = view.view_projection();
    // The center lands at clip origin; the top-left corner (+Y down the screen) at (-1, 1).
    let center = matrix * Vec4::new(400.0, 300.0, 0.0, 1.0);
    assert!(center.x.abs() < 1e-6 && center.y.abs() < 1e-6);
    let corner = matrix * Vec4::new(0.0, 0.0, 0.0, 1.0);
    assert!((corner.x + 1.0).abs() < 1e-6 && (corner.y - 1.0).abs() < 1e-6);
    // World z = height - near is the near plane (depth 0); height - far is the far plane (depth 1).
    assert!((matrix * Vec4::new(0.0, 0.0, 99.0, 1.0)).z.abs() < 1e-6);
    assert!(((matrix * Vec4::new(0.0, 0.0, -101.0, 1.0)).z - 1.0).abs() < 1e-6);

    // Camera-facing particles read their axes from the view's rows: the world's own right, up, and backward.
    let frame = view.camera_frame(4.0 / 3.0);
    assert_eq!(frame.view.row(0).truncate(), Vec3::X);
    assert_eq!(frame.view.row(1).truncate(), Vec3::Y);
    assert_eq!(frame.view.row(2).truncate(), Vec3::Z);
    assert_eq!(frame.eye, Vec3::new(400.0, 300.0, 100.0));

    assert!(TopDownView::new(Vec2::new(0.0, 1.0), Vec2::ZERO, camera).is_err());
    let inverted = TopDownCamera {
        far: 0.5,
        ..camera
    };
    assert!(TopDownView::new(Vec2::ONE, Vec2::ZERO, inverted).is_err());
}
