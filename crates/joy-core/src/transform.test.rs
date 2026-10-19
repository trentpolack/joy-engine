// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/transform.rs`.

use super::*;

fn close(left: Vec3, right: Vec3) -> bool {
    (left - right).abs().max_element() < 1e-5
}

/// Reference columns from the TypeScript `meshTransform` for the same input.
#[test]
// The reference values are copied verbatim from the TypeScript output so they stay traceable.
#[allow(clippy::excessive_precision)]
fn matrix_matches_typescript_mesh_transform() {
    let transform = Transform::new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.3, -0.7, 1.1), Vec3::new(2.0, 0.5, 1.5));
    let matrix = transform.matrix();
    let expected = [
        [0.6938588993097979, 1.363265973186846, 1.288435374475382],
        [-0.46887912125624864, 0.13183472674359614, 0.11301316062481151],
        [-0.02369029367795994, -1.0238034501272038, 1.0960224749032688],
        [1.0, 2.0, 3.0],
    ];
    for (column, values) in expected.iter().enumerate() {
        assert!(close(matrix.col(column).truncate(), Vec3::from_array(*values)), "column {column}");
    }
}

#[test]
fn euler_round_trip_preserves_rotation() {
    let rotation = Vec3::new(0.4, -0.2, 2.5);
    let recovered = euler_from_quat(quat_from_euler(rotation));
    assert!(close(recovered, rotation));
}

#[test]
fn attach_follows_parent_frame() {
    let parent = Transform::new(Vec3::new(5.0, 0.0, 0.0), Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0), Vec3::ONE);
    let child = parent.attach(Vec3::new(0.0, 0.0, 1.0), Vec3::splat(0.5));
    assert!(close(child.position, Vec3::new(6.0, 0.0, 0.0)));
    assert!(close(child.scale, Vec3::splat(0.5)));
}
