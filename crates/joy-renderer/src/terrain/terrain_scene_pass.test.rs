// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/terrain/terrain_scene_pass.rs`.

use super::*;

#[test]
fn local_views_keep_screen_error_invariant_under_uniform_scale() {
    // A uniformly scaled model: scale 2, translation (8, 4, -6), eye (10, 8, -4).
    let model =
        Mat4::from_cols_array(&[2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 8.0, 4.0, -6.0, 1.0]);
    let inverse = invert_matrix(model.as_dmat4()).expect("invertible");
    let eye = inverse.transform_point3(DVec3::new(10.0, 8.0, -4.0));
    assert_eq!(eye, DVec3::new(1.0, 2.0, 1.0));
    let scale = 100.0 * stretch_bound(model.as_dmat4()) * stretch_bound(inverse);
    assert!((scale - 100.0).abs() < 1e-12);
    assert!(invert_matrix(DMat4::ZERO).is_none());
}
