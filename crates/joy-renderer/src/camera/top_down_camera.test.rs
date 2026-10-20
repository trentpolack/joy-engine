// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/camera/top_down_camera.rs`.

use joy_core::math::Vec4;

use super::*;

#[test]
fn top_down_matrix_frames_the_viewport_and_maps_depth() {
    let camera = TopDownCamera {
        height: 100.0,
        near: 1.0,
        far: 201.0,
    };
    let matrix = top_down_orthographic(800.0, 600.0, Vec2::new(400.0, 300.0), camera).unwrap();
    // The center lands at clip origin; the top-left corner at (-1, 1).
    let center = matrix * Vec4::new(400.0, 300.0, 0.0, 1.0);
    assert!(center.x.abs() < 1e-6 && center.y.abs() < 1e-6);
    let corner = matrix * Vec4::new(0.0, 0.0, 0.0, 1.0);
    assert!((corner.x + 1.0).abs() < 1e-6 && (corner.y - 1.0).abs() < 1e-6);
    // World z = height - near is the near plane (depth 0); height - far is the far plane (depth 1).
    assert!((matrix * Vec4::new(0.0, 0.0, 99.0, 1.0)).z.abs() < 1e-6);
    assert!(((matrix * Vec4::new(0.0, 0.0, -101.0, 1.0)).z - 1.0).abs() < 1e-6);
    assert!(top_down_orthographic(0.0, 1.0, Vec2::ZERO, camera).is_err());
}

#[test]
fn transparent_triangles_sort_by_centroid_depth() {
    let triangle = |z: f32| {
        [ColorVertex {
            position: [0.0, 0.0, z],
            color: [1.0; 4],
        }; 3]
    };
    let vertices: Vec<ColorVertex> = [triangle(5.0), triangle(-3.0), triangle(1.0)].concat();
    let order: Vec<f32> = sort_top_down_transparent_triangles(&vertices).chunks(3).map(|t| t[0].position[2]).collect();
    assert_eq!(order, [-3.0, 1.0, 5.0]);
}
