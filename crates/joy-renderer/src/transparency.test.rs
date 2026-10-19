// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/transparency.rs`.

use super::*;
use crate::texture::TextureAsset;

/// A small triangle centered `depth` units in front of a camera at the origin looking down -Z.
fn colored(depth: f32) -> [ColorVertex; 3] {
    let vertex = |x: f32, y: f32| ColorVertex { position: [x, y, -depth], color: [1.0; 4] };
    [vertex(-0.1, 0.0), vertex(0.1, 0.0), vertex(0.0, 0.1)]
}

fn textured(depth: f32) -> [TexturedVertex; 3] {
    colored(depth).map(|vertex| TexturedVertex { position: vertex.position, color: vertex.color, uv: [0.0; 2] })
}

#[test]
fn particles_behind_and_in_front_of_a_glass_mesh_sort_around_it() {
    let sprite = TextureAsset::from_rgba8(1, 1, vec![255; 4]).unwrap();
    let far_particle = textured(15.0);
    let near_particle = textured(5.0);
    let particles = [far_particle, near_particle].concat();
    let draws = [TransparentDraw::Textured { texture: &sprite, vertices: &particles }];
    let smoke = colored(12.0);

    // One glass pane ten units away, between the two particles.
    let mut transient = Vec::new();
    let queue = build_transparent_queue(&[10.0], &smoke, &draws, Vec3::ZERO, Vec3::NEG_Z, &mut transient);

    let texture = sprite.id();
    assert_eq!(
        queue.steps,
        [
            TransparentStep::Textured { texture, vertices: 0..3 },
            TransparentStep::Colored(0..3),
            TransparentStep::Mesh(0),
            TransparentStep::Textured { texture, vertices: 3..6 },
        ]
    );
    assert_eq!(queue.mesh_order, [0]);
    assert_eq!(queue.textured_vertices[0].position[2], -15.0);
    assert_eq!(transient.len(), 3);
}

#[test]
fn adjacent_triangles_of_one_kind_merge_and_equal_depths_keep_their_order() {
    let mut transient = Vec::new();
    let triangles = [colored(9.0), colored(8.0), colored(8.0)].concat();
    let queue = build_transparent_queue(&[8.0, 3.0], &triangles, &[], Vec3::ZERO, Vec3::NEG_Z, &mut transient);
    assert_eq!(
        queue.steps,
        [
            TransparentStep::Colored(0..3),
            TransparentStep::Mesh(0),
            TransparentStep::Colored(3..9),
            TransparentStep::Mesh(1)
        ]
    );
    assert_eq!(queue.mesh_order, [0, 1]);
}
