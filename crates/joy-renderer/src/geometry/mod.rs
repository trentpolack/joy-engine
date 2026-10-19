// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Immediate triangle builders for arcade and effect geometry.
//!
//! Each function appends non-indexed triangles to a caller-owned [`ColorVertex`] list in the caller's logical world
//! units, with straight-alpha linear RGBA colors. Angles are radians. The lists feed [`TriangleRenderer`] frames or
//! the scene renderer's transient triangles. Math runs in `f64` and rounds once per vertex.
//!
//! [`TriangleRenderer`]: crate::TriangleRenderer

use std::f64::consts::{
    PI,
    TAU,
};

use crate::ColorVertex;
use joy_core::math::deterministic;
use joy_core::{
    LinearColor,
    ShadedMesh3D,
};

/// Append one vertex at `(x, y, z)`.
fn push(vertices: &mut Vec<ColorVertex>, x: f64, y: f64, z: f64, color: LinearColor) {
    vertices.push(ColorVertex { position: [x as f32, y as f32, z as f32], color: color.to_array() });
}

/// Append an equilateral triangle whose corners lie `radius` from its center.
pub fn add_triangle(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    color: LinearColor,
    rotation: f32,
    z: f32,
) {
    let (x, y, radius, rotation, z) =
        (f64::from(x), f64::from(y), f64::from(radius), f64::from(rotation), f64::from(z));
    for index in 0..3 {
        let angle = rotation + ((f64::from(index) * TAU) / 3.0);
        push(vertices, x + (deterministic::cos(angle) * radius), y + (deterministic::sin(angle) * radius), z, color);
    }
}

/// Append a square as two triangles. `radius` reaches a corner, not an edge; a rotation of π/4 is axis-aligned.
pub fn add_quad(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    color: LinearColor,
    rotation: f32,
    z: f32,
) {
    let (x, y, radius, rotation, z) =
        (f64::from(x), f64::from(y), f64::from(radius), f64::from(rotation), f64::from(z));
    let corner = |index: u32| {
        let angle = rotation + ((f64::from(index) * TAU) / 4.0);
        (x + (deterministic::cos(angle) * radius), y + (deterministic::sin(angle) * radius))
    };
    for index in [0, 1, 2, 0, 2, 3] {
        let (corner_x, corner_y) = corner(index);
        push(vertices, corner_x, corner_y, z, color);
    }
}

/// Append a triangle fan approximating a filled circle with `segments` slices.
pub fn add_circle(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    color: LinearColor,
    segments: u32,
    z: f32,
) {
    let (x, y, radius, z) = (f64::from(x), f64::from(y), f64::from(radius), f64::from(z));
    let segments_f = f64::from(segments);
    for index in 0..segments {
        let first = (f64::from(index) / segments_f) * TAU;
        let second = (f64::from(index + 1) / segments_f) * TAU;
        push(vertices, x, y, z, color);
        push(vertices, x + (deterministic::cos(first) * radius), y + (deterministic::sin(first) * radius), z, color);
        push(vertices, x + (deterministic::cos(second) * radius), y + (deterministic::sin(second) * radius), z, color);
    }
}

/// Append a flat-shaded mesh scaled, rotated about Z, and offset to `(x, y, z)`. Each triangle's shade multiplies the
/// color's RGB, which may exceed one in the HDR scene (clipping channels separately would shift hue); alpha is
/// unchanged.
#[allow(clippy::too_many_arguments)] // Games call every builder with positional shape values.
pub fn add_mesh(
    vertices: &mut Vec<ColorVertex>,
    mesh: &ShadedMesh3D,
    x: f32,
    y: f32,
    scale: f32,
    rotation: f32,
    color: LinearColor,
    z: f32,
) {
    let (x, y, scale, rotation, z) = (f64::from(x), f64::from(y), f64::from(scale), f64::from(rotation), f64::from(z));
    let (sine, cosine) = deterministic::sin_cos(rotation);
    for triangle in &mesh.triangles {
        let shade = triangle[9];
        let channel = |value: f32| (f64::from(value) * shade) as f32;
        let shaded = LinearColor::new(channel(color.red), channel(color.green), channel(color.blue), color.alpha);
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
#[allow(clippy::too_many_arguments)] // Games call every builder with positional shape values.
pub fn add_polygon(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    color: LinearColor,
    sides: usize,
    rotation: f32,
    z: f32,
) {
    add_radial_polygon(vertices, x, y, radius, color, &vec![1.0; sides], rotation, z);
}

/// Append a triangle fan from evenly spaced radial multipliers; a sample of 1 lies at `radius`.
#[allow(clippy::too_many_arguments)] // Games call every builder with positional shape values.
pub fn add_radial_polygon(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    color: LinearColor,
    shape: &[f32],
    rotation: f32,
    z: f32,
) {
    add_gradient_radial_polygon(vertices, x, y, radius, color, color, shape, rotation, z);
}

/// Append an irregular triangle fan whose center and edge colors interpolate into a soft radial falloff.
#[allow(clippy::too_many_arguments)] // Games call every builder with positional shape values.
pub fn add_gradient_radial_polygon(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    center_color: LinearColor,
    edge_color: LinearColor,
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
            x + (deterministic::cos(first_angle) * radius * first),
            y + (deterministic::sin(first_angle) * radius * first),
            z,
            edge_color,
        );
        push(
            vertices,
            x + (deterministic::cos(second_angle) * radius * second),
            y + (deterministic::sin(second_angle) * radius * second),
            z,
            edge_color,
        );
    }
}

/// Append an annulus as two triangles per segment. `width` is the half thickness; the inner radius clamps to zero.
#[allow(clippy::too_many_arguments)] // Games call every builder with positional shape values.
pub fn add_ring(
    vertices: &mut Vec<ColorVertex>,
    x: f32,
    y: f32,
    radius: f32,
    width: f32,
    color: LinearColor,
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
            push(
                vertices,
                x + (deterministic::cos(angle) * corner_radius),
                y + (deterministic::sin(angle) * corner_radius),
                z,
                color,
            );
        }
    }
}

/// Append a butt-ended segment from `a` to `b` as two triangles; `width` is the full thickness.
#[allow(clippy::too_many_arguments)] // Games call every builder with positional shape values.
pub fn add_line(
    vertices: &mut Vec<ColorVertex>,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    width: f32,
    color: LinearColor,
    z: f32,
) {
    let (ax, ay, bx, by, width, z) =
        (f64::from(ax), f64::from(ay), f64::from(bx), f64::from(by), f64::from(width), f64::from(z));
    let angle = deterministic::atan2(by - ay, bx - ax) + (PI / 2.0);
    let offset_x = (deterministic::cos(angle) * width) / 2.0;
    let offset_y = (deterministic::sin(angle) * width) / 2.0;
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
#[path = "mod.test.rs"]
mod tests;
