// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Immediate triangle builders for arcade and effect geometry.
//!
//! Each function appends non-indexed triangles to a caller-owned [`ColorVertex`] list in the caller's logical world
//! units, with straight-alpha linear RGBA colors. Angles are radians. The lists feed [`TriangleRenderer`] frames or
//! the scene renderer's transient triangles. Math runs in `f64` and rounds once per vertex.
//!
//! [`TriangleRenderer`]: crate::rendering::TriangleRenderer

use std::f64::consts::{
    PI,
    TAU,
};

use crate::core::{
    Rgba,
    ShadedMesh3D,
};
use crate::rendering::ColorVertex;

/// Append one vertex at `(x, y, z)`.
fn push(vertices: &mut Vec<ColorVertex>, x: f64, y: f64, z: f64, color: Rgba) {
    vertices.push(ColorVertex { position: [x as f32, y as f32, z as f32], color });
}

/// Append an equilateral triangle whose corners lie `radius` from its center.
pub fn add_triangle(vertices: &mut Vec<ColorVertex>, x: f32, y: f32, radius: f32, color: Rgba, rotation: f32, z: f32) {
    let (x, y, radius, rotation, z) =
        (f64::from(x), f64::from(y), f64::from(radius), f64::from(rotation), f64::from(z));
    for index in 0..3 {
        let angle = rotation + ((f64::from(index) * TAU) / 3.0);
        push(vertices, x + (angle.cos() * radius), y + (angle.sin() * radius), z, color);
    }
}

/// Append a square as two triangles. `radius` reaches a corner, not an edge; a rotation of π/4 is axis-aligned.
pub fn add_quad(vertices: &mut Vec<ColorVertex>, x: f32, y: f32, radius: f32, color: Rgba, rotation: f32, z: f32) {
    let (x, y, radius, rotation, z) =
        (f64::from(x), f64::from(y), f64::from(radius), f64::from(rotation), f64::from(z));
    let corner = |index: u32| {
        let angle = rotation + ((f64::from(index) * TAU) / 4.0);
        (x + (angle.cos() * radius), y + (angle.sin() * radius))
    };
    for index in [0, 1, 2, 0, 2, 3] {
        let (corner_x, corner_y) = corner(index);
        push(vertices, corner_x, corner_y, z, color);
    }
}

/// Append a triangle fan approximating a filled circle with `segments` slices.
pub fn add_circle(vertices: &mut Vec<ColorVertex>, x: f32, y: f32, radius: f32, color: Rgba, segments: u32, z: f32) {
    let (x, y, radius, z) = (f64::from(x), f64::from(y), f64::from(radius), f64::from(z));
    let segments_f = f64::from(segments);
    for index in 0..segments {
        let first = (f64::from(index) / segments_f) * TAU;
        let second = (f64::from(index + 1) / segments_f) * TAU;
        push(vertices, x, y, z, color);
        push(vertices, x + (first.cos() * radius), y + (first.sin() * radius), z, color);
        push(vertices, x + (second.cos() * radius), y + (second.sin() * radius), z, color);
    }
}

/// Append a flat-shaded mesh scaled, rotated about Z, and offset to `(x, y, z)`. Each triangle's shade multiplies the
/// color's RGB (capped at one); alpha is unchanged.
#[allow(clippy::too_many_arguments)] // Mirrors the TypeScript signature that games call with positional values.
pub fn add_mesh(
    vertices: &mut Vec<ColorVertex>,
    mesh: &ShadedMesh3D,
    x: f32,
    y: f32,
    scale: f32,
    rotation: f32,
    color: Rgba,
    z: f32,
) {
    let (x, y, scale, rotation, z) = (f64::from(x), f64::from(y), f64::from(scale), f64::from(rotation), f64::from(z));
    let (sine, cosine) = rotation.sin_cos();
    for triangle in &mesh.triangles {
        let shade = triangle[9];
        let channel = |value: f32| (f64::from(value) * shade).min(1.0) as f32;
        let shaded = [channel(color[0]), channel(color[1]), channel(color[2]), color[3]];
        for corner in triangle[..9].as_chunks::<3>().0 {
            let local_x = corner[0] * scale;
            let local_y = corner[1] * scale;
            push(
                vertices,
                x + (local_x * cosine) - (local_y * sine),
                y + (local_x * sine) + (local_y * cosine),
                z + (corner[2] * scale),
                shaded,
            );
        }
    }
}

/// Append a regular polygon with `sides` equal radial samples.
#[allow(clippy::too_many_arguments)] // Mirrors the TypeScript signature that games call with positional values.
pub fn add_polygon(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    color: Rgba,
    sides: usize,
    rotation: f32,
    z: f32,
) {
    add_radial_polygon(vertices, x, y, radius, color, &vec![1.0; sides], rotation, z);
}

/// Append a triangle fan from evenly spaced radial multipliers; a sample of 1 lies at `radius`.
#[allow(clippy::too_many_arguments)] // Mirrors the TypeScript signature that games call with positional values.
pub fn add_radial_polygon(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    color: Rgba,
    shape: &[f32],
    rotation: f32,
    z: f32,
) {
    add_gradient_radial_polygon(vertices, x, y, radius, color, color, shape, rotation, z);
}

/// Append an irregular triangle fan whose center and edge colors interpolate into a soft radial falloff.
#[allow(clippy::too_many_arguments)] // Mirrors the TypeScript signature that games call with positional values.
pub fn add_gradient_radial_polygon(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    center_color: Rgba,
    edge_color: Rgba,
    shape: &[f32],
    rotation: f32,
    z: f32,
) {
    let (x, y, radius, rotation, z) =
        (f64::from(x), f64::from(y), f64::from(radius), f64::from(rotation), f64::from(z));
    let count = shape.len();
    for index in 0..count {
        let first_angle = rotation + (((index as f64) / (count as f64)) * PI * 2.0);
        let second_angle = rotation + ((((index + 1) as f64) / (count as f64)) * PI * 2.0);
        let first = f64::from(shape[index]);
        let second = f64::from(shape[(index + 1) % count]);
        push(vertices, x, y, z, center_color);
        push(
            vertices,
            x + (first_angle.cos() * radius * first),
            y + (first_angle.sin() * radius * first),
            z,
            edge_color,
        );
        push(
            vertices,
            x + (second_angle.cos() * radius * second),
            y + (second_angle.sin() * radius * second),
            z,
            edge_color,
        );
    }
}

/// Append an annulus as two triangles per segment. `width` is the half thickness; the inner radius clamps to zero.
#[allow(clippy::too_many_arguments)] // Mirrors the TypeScript signature that games call with positional values.
pub fn add_ring(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    width: f32,
    color: Rgba,
    segments: u32,
    z: f32,
) {
    let (x, y, radius, width, z) = (f64::from(x), f64::from(y), f64::from(radius), f64::from(width), f64::from(z));
    let inner = (radius - width).max(0.0);
    let outer = radius + width;
    for index in 0..segments {
        let first = (f64::from(index) / f64::from(segments)) * PI * 2.0;
        let second = (f64::from(index + 1) / f64::from(segments)) * PI * 2.0;
        let corners = [(inner, first), (outer, first), (outer, second), (inner, second)];
        for corner in [0, 1, 2, 0, 2, 3] {
            let (corner_radius, angle) = corners[corner];
            push(vertices, x + (angle.cos() * corner_radius), y + (angle.sin() * corner_radius), z, color);
        }
    }
}

/// Append a butt-ended segment from `a` to `b` as two triangles; `width` is the full thickness.
#[allow(clippy::too_many_arguments)] // Mirrors the TypeScript signature that games call with positional values.
pub fn add_line(vertices: &mut Vec<ColorVertex>, ax: f32, ay: f32, bx: f32, by: f32, width: f32, color: Rgba, z: f32) {
    let (ax, ay, bx, by, width, z) =
        (f64::from(ax), f64::from(ay), f64::from(bx), f64::from(by), f64::from(width), f64::from(z));
    let angle = (by - ay).atan2(bx - ax) + (PI / 2.0);
    let offset_x = (angle.cos() * width) / 2.0;
    let offset_y = (angle.sin() * width) / 2.0;
    let corners = [
        (ax - offset_x, ay - offset_y),
        (ax + offset_x, ay + offset_y),
        (bx + offset_x, by + offset_y),
        (bx - offset_x, by - offset_y),
    ];
    for corner in [0, 1, 2, 0, 2, 3] {
        let (corner_x, corner_y) = corners[corner];
        push(vertices, corner_x, corner_y, z, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];

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
    fn geometry_matches_typescript_values() {
        // Recorded from the TypeScript helpers: addQuad(v, 10, 20, 2, c) (default rotation π/4) and
        // addLine(v, 0, 0, 3, 4, 2, c, 0).
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
        add_mesh(&mut vertices, &mesh, 10.0, 20.0, 2.0, std::f32::consts::FRAC_PI_2, [0.8, 1.0, 0.2, 0.5], 3.0);
        assert_eq!(vertices.len(), 3);
        // A quarter turn maps local +X to world +Y.
        let first = vertices[0].position;
        assert!((first[0] - 10.0).abs() < 1e-5 && (first[1] - 22.0).abs() < 1e-5 && (first[2] - 4.0).abs() < 1e-6);
        assert_eq!(vertices[0].color, [0.4, 0.5, 0.1, 0.5]);
        // Shades above one brighten but cap at full intensity.
        let bright = ShadedMesh3D {
            triangles: vec![[0.0; 9].into_iter().chain([2.0]).collect::<Vec<_>>().try_into().unwrap()],
            ..mesh
        };
        let mut vertices = Vec::new();
        add_mesh(&mut vertices, &bright, 0.0, 0.0, 1.0, 0.0, [0.8, 0.2, 0.0, 1.0], 0.0);
        assert_eq!(vertices[0].color, [1.0, 0.4, 0.0, 1.0]);
    }

    #[test]
    fn gradient_fans_interpolate_center_to_edge() {
        let mut vertices = Vec::new();
        let edge = [0.0, 0.0, 0.0, 0.0];
        add_gradient_radial_polygon(&mut vertices, 0.0, 0.0, 1.0, WHITE, edge, &[1.0, 0.5, 1.0], 0.0, 0.0);
        assert_eq!(vertices.len(), 9);
        assert_eq!(vertices[0].color, WHITE);
        assert_eq!(vertices[1].color, edge);
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
}
