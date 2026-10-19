// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/frame.rs`.

use super::*;

#[test]
fn transparent_triangles_sort_back_to_front() {
    let triangle = |z: f32| {
        [
            ColorVertex { position: [0.0, 0.0, z], color: [1.0; 4] },
            ColorVertex { position: [1.0, 0.0, z], color: [1.0; 4] },
            ColorVertex { position: [0.0, 1.0, z], color: [1.0; 4] },
        ]
    };
    let vertices: Vec<ColorVertex> = [triangle(5.0), triangle(-5.0), triangle(0.0)].concat();
    // Camera on +Z looking toward -Z: the farthest (most negative Z) triangle draws first.
    let sorted = sort_transparent_triangles(&vertices, Vec3::Z);
    let order: Vec<f32> = sorted.as_chunks::<3>().0.iter().map(|triangle| triangle[0].position[2]).collect();
    assert_eq!(order, [-5.0, 0.0, 5.0]);
}

#[test]
fn ambient_default_depends_on_environment() {
    assert_eq!(FrameInputs::default().resolved_ambient(), DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT);
}
