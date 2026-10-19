// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/geometry/mod.rs`.

use super::*;

const WHITE: LinearColor = LinearColor::WHITE;

#[test]
fn builders_emit_whole_triangles_at_their_depth() {
    let mut vertices = Vec::new();
    add_triangle(&mut vertices, 0.0, 0.0, 1.0, WHITE, 0.0, 2.0);
    add_quad(&mut vertices, 0.0, 0.0, 1.0, WHITE, std::f32::consts::FRAC_PI_4, 2.0);
    add_circle(&mut vertices, 0.0, 0.0, 1.0, WHITE, 12, 2.0);
    add_polygon(&mut vertices, 0.0, 0.0, 1.0, WHITE, 5, 0.0, 2.0);
    add_ring(&mut vertices, 0.0, 0.0, 1.0, 0.25, WHITE, 8, 2.0);
    add_line(&mut vertices, 0.0, 0.0, 3.0, 4.0, 2.0, WHITE, 2.0);
    assert_eq!(vertices.len(), 3 + 6 + (12 * 3) + (5 * 3) + (8 * 6) + 6);
    assert!(vertices.iter().all(|vertex| vertex.position[2] == 2.0));
}

#[test]
fn geometry_matches_recorded_values() {
    // Recorded reference corners: a size 2 quad at (10, 20) rotated π/4, and a width 2 line from (0, 0) to (3, 4).
    let mut quad = Vec::new();
    add_quad(&mut quad, 10.0, 20.0, 2.0, WHITE, std::f32::consts::FRAC_PI_4, 0.0);
    let expected_quad = [[11.414_213, 21.414_213], [8.585_787, 21.414_213], [8.585_787, 18.585_787]];
    for (vertex, expected) in quad.iter().zip(expected_quad) {
        assert!((vertex.position[0] - expected[0]).abs() < 1e-5 && (vertex.position[1] - expected[1]).abs() < 1e-5);
    }
    let mut line = Vec::new();
    add_line(&mut line, 0.0, 0.0, 3.0, 4.0, 2.0, WHITE, 0.0);
    let expected_line = [[0.8, -0.6], [-0.8, 0.6], [2.2, 4.6]];
    for (vertex, expected) in line.iter().zip(expected_line) {
        assert!((vertex.position[0] - expected[0]).abs() < 1e-5 && (vertex.position[1] - expected[1]).abs() < 1e-5);
    }
}

#[test]
fn meshes_scale_rotate_offset_and_shade() {
    let mesh =
        ShadedMesh3D { name: "wedge".into(), triangles: vec![[1.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5]] };
    let mut vertices = Vec::new();
    add_mesh(
        &mut vertices,
        &mesh,
        10.0,
        20.0,
        2.0,
        std::f32::consts::FRAC_PI_2,
        LinearColor::new(0.8, 1.0, 0.2, 0.5),
        3.0,
    );
    assert_eq!(vertices.len(), 3);
    // A quarter turn maps local +X to world +Y.
    let first = vertices[0].position;
    assert!((first[0] - 10.0).abs() < 1e-5 && (first[1] - 22.0).abs() < 1e-5 && (first[2] - 4.0).abs() < 1e-6);
    assert_eq!(vertices[0].color, [0.4, 0.5, 0.1, 0.5]);
    // Shades above one brighten every channel by the same factor, keeping the hue.
    let bright = ShadedMesh3D {
        triangles: vec![[0.0; 9].into_iter().chain([2.0]).collect::<Vec<_>>().try_into().unwrap()],
        ..mesh
    };
    let mut vertices = Vec::new();
    add_mesh(&mut vertices, &bright, 0.0, 0.0, 1.0, 0.0, LinearColor::new(0.8, 0.2, 0.0, 1.0), 0.0);
    assert_eq!(vertices[0].color, [1.6, 0.4, 0.0, 1.0]);
}

#[test]
fn gradient_fans_interpolate_center_to_edge() {
    let mut vertices = Vec::new();
    let edge = LinearColor::TRANSPARENT;
    add_gradient_radial_polygon(&mut vertices, 0.0, 0.0, 1.0, WHITE, edge, &[1.0, 0.5, 1.0], 0.0, 0.0);
    assert_eq!(vertices.len(), 9);
    assert_eq!(vertices[0].color, WHITE.to_array());
    assert_eq!(vertices[1].color, edge.to_array());
    // The second sample halves the radius of the second corner of the first slice.
    let second_corner = Vec2Length::length(vertices[2].position);
    assert!((second_corner - 0.5).abs() < 1e-6);
}

/// Planar length of a vertex position, for readability in assertions.
trait Vec2Length {
    fn length(self) -> f32;
}

impl Vec2Length for [f32; 3] {
    fn length(self) -> f32 {
        self[0].hypot(self[1])
    }
}
