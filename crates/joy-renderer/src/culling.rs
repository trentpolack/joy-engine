// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Conservative visibility tests shared by the camera, shadow views, and point-light selection.
//!
//! Every test errs toward "visible": a volume is reported outside a view only when all of it lies beyond one clip plane,
//! so nothing that could touch the view is ever skipped. Clip space uses zero-to-one depth.

use joy_core::math::{
    Mat4,
    Vec3,
    Vec4,
};

use crate::mesh::Bounds;

/// Bounds that contain nothing: transforming any point into them widens them to that point.
pub(crate) const EMPTY_BOUNDS: Bounds = Bounds {
    min: Vec3::splat(f32::INFINITY),
    max: Vec3::splat(f32::NEG_INFINITY),
};

/// Whether local `bounds` transformed by `model` lie entirely outside one of `view_projection`'s six clip planes.
pub(crate) fn outside_frustum(bounds: Bounds, model: Mat4, view_projection: Mat4) -> bool {
    let matrix = view_projection * model;
    let mut outside = [true; 6];
    for corner in bounds.corners() {
        let clip = matrix * corner.extend(1.0);
        for (plane, distance) in clip_plane_distances(clip).iter().enumerate() {
            outside[plane] = outside[plane] && *distance < 0.0;
        }
    }
    outside.iter().any(|plane| *plane)
}

/// Whether world-space `bounds` lie entirely outside one of `view_projection`'s clip planes.
pub(crate) fn bounds_outside_view(bounds: Bounds, view_projection: Mat4) -> bool {
    outside_frustum(bounds, Mat4::IDENTITY, view_projection)
}

/// Whether the sphere at `center` with `radius` lies entirely outside one of `view_projection`'s clip planes.
///
/// Each plane is extracted from the matrix rows (Gribb and Hartmann) and normalized, so the radius compares in world
/// units for perspective and orthographic projections alike.
pub(crate) fn sphere_outside_view(center: Vec3, radius: f32, view_projection: Mat4) -> bool {
    let rows = [view_projection.row(0), view_projection.row(1), view_projection.row(2), view_projection.row(3)];
    let planes: [Vec4; 6] =
        [rows[3] + rows[0], rows[3] - rows[0], rows[3] + rows[1], rows[3] - rows[1], rows[2], rows[3] - rows[2]];
    let point = center.extend(1.0);
    planes.iter().any(|plane| {
        let length = plane.truncate().length();
        length > 0.0 && (plane.dot(point) / length) < -radius
    })
}

/// Whether the sphere at `center` with `radius` overlaps `bounds` (touching counts).
pub(crate) fn sphere_touches_bounds(center: Vec3, radius: f32, bounds: Bounds) -> bool {
    let nearest = center.clamp(bounds.min, bounds.max);
    nearest.distance_squared(center) <= radius * radius
}

/// Transform local bounds into conservative world bounds.
pub(crate) fn world_bounds(bounds: Bounds, model: Mat4) -> Bounds {
    bounds.corners().iter().fold(EMPTY_BOUNDS, |result, corner| {
        let point = model.transform_point3(*corner);
        Bounds {
            min: result.min.min(point),
            max: result.max.max(point),
        }
    })
}

/// Signed distances of a clip-space point to the six frustum planes: left, right, bottom, top, near, and far.
fn clip_plane_distances(clip: Vec4) -> [f32; 6] {
    [clip.x + clip.w, clip.w - clip.x, clip.y + clip.w, clip.w - clip.y, clip.z, clip.w - clip.z]
}

#[cfg(test)]
#[path = "culling.test.rs"]
mod tests;
