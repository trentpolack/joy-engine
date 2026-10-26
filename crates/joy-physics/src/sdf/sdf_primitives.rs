// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Analytical signed distance functions with gradients, evaluated in a shape's local frame.
//!
//! Every function returns the signed distance (negative inside) and its gradient, a unit vector pointing away from the
//! nearest surface. Symmetric primitives use +Y as their axis; the torus lies in the XZ plane. Values are meters in the
//! solver's scalar math, so native and WebAssembly builds evaluate them bit for bit alike.

use rapier3d::math::{
    Real,
    Vector,
};
use rapier3d::parry::shape::HeightField;

/// Distances below this are treated as zero when normalizing a gradient direction.
const DIRECTION_EPSILON: Real = 1.0e-9;

/// One evaluation of a field: signed distance in meters and the gradient of that distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FieldSample {
    /// Signed distance; negative inside the shape.
    pub distance: Real,
    /// Gradient of the distance. Unit length for exact fields; approximately unit length for baked grids.
    pub gradient: Vector,
}

/// Axis-aligned box centered on the origin.
pub(crate) fn box_field(point: Vector, half_extents: Vector) -> FieldSample {
    let offset = point.abs() - half_extents;
    let outside = offset.max(Vector::ZERO);
    let outside_length = outside.length();
    let inside_distance = offset.max_element().min(0.0);

    // Outside, the gradient points from the closest point on the box; inside, along the nearest face's normal.
    let gradient = if outside_length > 0.0 {
        (outside / outside_length) * signs(point)
    } else {
        let axis = largest_axis(offset);
        let mut direction = Vector::ZERO;
        direction[axis] = sign(point[axis]);
        direction
    };
    FieldSample {
        distance: outside_length + inside_distance,
        gradient,
    }
}

/// Sphere centered on the origin.
pub(crate) fn sphere_field(point: Vector, radius: Real) -> FieldSample {
    let length = point.length();
    let gradient = if length > DIRECTION_EPSILON { point / length } else { Vector::Y };
    FieldSample {
        distance: length - radius,
        gradient,
    }
}

/// Capsule along Y: a segment from `-half_length` to `half_length` swept by `radius`.
pub(crate) fn capsule_field(point: Vector, radius: Real, half_length: Real) -> FieldSample {
    let along_axis = point.y.clamp(-half_length, half_length);
    sphere_field(Vector::new(point.x, point.y - along_axis, point.z), radius)
}

/// Solid cylinder along Y with flat caps at `±half_height`.
pub(crate) fn cylinder_field(point: Vector, radius: Real, half_height: Real) -> FieldSample {
    let radial = ((point.x * point.x) + (point.z * point.z)).sqrt();
    let radial_offset = radial - radius;
    let height_offset = point.y.abs() - half_height;
    let radial_outside = radial_offset.max(0.0);
    let height_outside = height_offset.max(0.0);
    let outside = ((radial_outside * radial_outside) + (height_outside * height_outside)).sqrt();

    let radial_direction = radial_direction(point, radial);
    let gradient = if outside > 0.0 {
        (radial_direction * (radial_outside / outside))
            + Vector::new(0.0, sign(point.y) * (height_outside / outside), 0.0)
    } else if radial_offset > height_offset {
        radial_direction
    } else {
        Vector::new(0.0, sign(point.y), 0.0)
    };
    FieldSample {
        distance: outside + radial_offset.max(height_offset).min(0.0),
        gradient,
    }
}

/// Torus around Y: a circle of `major_radius` in the XZ plane swept by `minor_radius`.
pub(crate) fn torus_field(point: Vector, major_radius: Real, minor_radius: Real) -> FieldSample {
    let radial = ((point.x * point.x) + (point.z * point.z)).sqrt();
    let ring_offset = radial - major_radius;
    let length = ((ring_offset * ring_offset) + (point.y * point.y)).sqrt();

    let radial_direction = radial_direction(point, radial);
    let gradient = if length > DIRECTION_EPSILON {
        (radial_direction * (ring_offset / length)) + Vector::new(0.0, point.y / length, 0.0)
    } else {
        radial_direction
    };
    FieldSample {
        distance: length - minor_radius,
        gradient,
    }
}

/// Half-space whose boundary plane passes through the origin with the unit outward `normal`.
pub(crate) fn half_space_field(point: Vector, normal: Vector) -> FieldSample {
    FieldSample {
        distance: point.dot(normal),
        gradient: normal,
    }
}

/// A heightfield measured to the plane of the triangle under the point (or under its clamp onto the grid): exact for
/// points nearest that triangle, and the triangle's upward normal as the gradient. Cells must use the zigzag split
/// (diagonal from the cell's -X, -Z corner to its +X, +Z corner), as terrain heightfields do.
pub(crate) fn heightfield_field(point: Vector, heightfield: &HeightField) -> FieldSample {
    let (row, column) = heightfield.closest_cell_at_point(point);
    let (first, second) = heightfield.triangles_at(row, column);
    let x0 = heightfield.x_at(column);
    let z0 = heightfield.z_at(row);
    let fraction_x = (point.x - x0) / heightfield.cell_width();
    let fraction_z = (point.z - z0) / heightfield.cell_height();

    // The zigzag split's first triangle holds the +Z side of the diagonal and the second the +X side.
    let triangle = if fraction_x >= fraction_z { second.or(first) } else { first.or(second) };
    let Some(triangle) = triangle else {
        return FieldSample {
            distance: Real::MAX,
            gradient: Vector::Y,
        };
    };
    let mut normal = (triangle.b - triangle.a).cross(triangle.c - triangle.a);
    if normal.y < 0.0 {
        normal = -normal;
    }
    let length = normal.length();
    if length <= DIRECTION_EPSILON {
        return FieldSample {
            distance: point.y - triangle.a.y,
            gradient: Vector::Y,
        };
    }
    let normal = normal / length;
    FieldSample {
        distance: (point - triangle.a).dot(normal),
        gradient: normal,
    }
}

/// `-1` for negative values and `+1` otherwise, so points on a symmetry plane still receive a direction.
fn sign(value: Real) -> Real {
    if value < 0.0 { -1.0 } else { 1.0 }
}

/// Component-wise [`sign`].
fn signs(value: Vector) -> Vector {
    Vector::new(sign(value.x), sign(value.y), sign(value.z))
}

/// Index of the largest component; ties prefer the later axis, matching the reference implementation.
fn largest_axis(value: Vector) -> usize {
    if value.x > value.y {
        if value.x > value.z { 0 } else { 2 }
    } else if value.y > value.z {
        1
    } else {
        2
    }
}

/// Unit direction away from the Y axis in the XZ plane, or +X on the axis itself.
fn radial_direction(point: Vector, radial: Real) -> Vector {
    if radial > DIRECTION_EPSILON { Vector::new(point.x / radial, 0.0, point.z / radial) } else { Vector::X }
}
