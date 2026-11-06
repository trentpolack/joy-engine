// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/transparency.rs`.

use super::*;

/// A small triangle centered `depth` units in front of a camera at the origin looking down -Z.
fn colored(depth: f32) -> [ColorVertex; 3] {
    let vertex = |x: f32, y: f32| ColorVertex {
        position: [x, y, -depth],
        color: [1.0; 4],
    };
    [vertex(-0.1, 0.0), vertex(0.1, 0.0), vertex(0.0, 0.1)]
}

#[test]
fn particle_batches_behind_and_in_front_of_a_glass_mesh_sort_around_it() {
    let smoke = colored(12.0);

    // One glass pane ten units away, between a far and a near particle batch.
    let mut transient = Vec::new();
    let queue =
        build_transparent_queue(&[10.0], &smoke, [15.0, 5.0].into_iter(), Vec3::ZERO, Vec3::NEG_Z, &mut transient);
    assert_eq!(
        queue.steps,
        [
            TransparentStep::Particles(0),
            TransparentStep::Colored(0..3),
            TransparentStep::Mesh(0),
            TransparentStep::Particles(1),
        ]
    );
    assert_eq!(queue.mesh_order, [0]);
    assert_eq!(transient.len(), 3);
}

#[test]
fn adjacent_triangles_merge_and_equal_depths_keep_their_order() {
    let mut transient = Vec::new();
    let triangles = [colored(9.0), colored(8.0), colored(8.0)].concat();
    let queue =
        build_transparent_queue(&[8.0, 3.0], &triangles, [8.0].into_iter(), Vec3::ZERO, Vec3::NEG_Z, &mut transient);
    assert_eq!(
        queue.steps,
        [
            TransparentStep::Colored(0..3),
            TransparentStep::Mesh(0),
            TransparentStep::Colored(3..9),
            TransparentStep::Particles(0),
            TransparentStep::Mesh(1)
        ]
    );
    assert_eq!(queue.mesh_order, [0, 1]);
}
