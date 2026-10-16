// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Fixed top-down orthographic camera for arcade games.

use glam::{Mat4, Vec2};

use crate::rendering::{ColorVertex, RenderError};

/// Camera placement for [`top_down_orthographic`]: the camera sits at world `height` on +Z looking down -Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopDownCamera {
    /// Camera Z in world units.
    pub height: f32,
    /// Near distance in front of the camera, in world units.
    pub near: f32,
    /// Far distance in front of the camera, in world units; greater than `near`.
    pub far: f32,
}

/// Column-major orthographic view-projection for a camera looking down -Z at `center`.
///
/// X and Y use the caller's logical world units (one per CSS pixel in the arcade games), +Y is down the screen, and
/// display density is deliberately absent so it never changes framing. Depth maps `near..far` in front of the camera to
/// zero-to-one clip depth on every backend. Fails for an empty viewport or a non-positive depth range.
pub fn top_down_orthographic(
    width: f32,
    height: f32,
    center: Vec2,
    camera: TopDownCamera,
) -> Result<Mat4, RenderError> {
    if width <= 0.0 || height <= 0.0 {
        return Err(RenderError::InvalidFrame("logical viewport dimensions must be positive".into()));
    }
    let depth_range = camera.far - camera.near;
    if depth_range <= 0.0 {
        return Err(RenderError::InvalidFrame("camera far distance must exceed its near distance".into()));
    }
    Ok(Mat4::from_cols_array(&[
        2.0 / width,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / height,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0 / depth_range,
        0.0,
        (-center.x * 2.0) / width,
        (center.y * 2.0) / height,
        (camera.height - camera.near) / depth_range,
        1.0,
    ]))
}

/// Copy triangles ordered back to front for a top-down camera: smaller centroid Z is farther away.
///
/// Equal depths keep authored order; trailing vertices that do not complete a triangle are dropped. Centroid ordering
/// suits parallel effect planes, not intersecting transparent meshes.
pub fn sort_top_down_transparent_triangles(vertices: &[ColorVertex]) -> Vec<ColorVertex> {
    let mut triangles: Vec<(f32, usize)> = vertices
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .map(|(index, triangle)| {
            ((triangle[0].position[2] + triangle[1].position[2] + triangle[2].position[2]) / 3.0, index)
        })
        .collect();
    triangles.sort_by(|left, right| left.0.total_cmp(&right.0));
    triangles.into_iter().flat_map(|(_, index)| vertices[index * 3..(index * 3) + 3].iter().copied()).collect()
}

#[cfg(test)]
mod tests {
    use glam::Vec4;

    use super::*;

    #[test]
    fn top_down_matrix_frames_the_viewport_and_maps_depth() {
        let camera = TopDownCamera { height: 100.0, near: 1.0, far: 201.0 };
        let matrix = top_down_orthographic(800.0, 600.0, Vec2::new(400.0, 300.0), camera).unwrap();
        // The center lands at clip origin; the top-left corner at (-1, 1).
        let center = matrix * Vec4::new(400.0, 300.0, 0.0, 1.0);
        assert!(center.x.abs() < 1e-6 && center.y.abs() < 1e-6);
        let corner = matrix * Vec4::new(0.0, 0.0, 0.0, 1.0);
        assert!((corner.x + 1.0).abs() < 1e-6 && (corner.y - 1.0).abs() < 1e-6);
        // World z = height - near is the near plane (depth 0); height - far is the far plane (depth 1).
        assert!((matrix * Vec4::new(0.0, 0.0, 99.0, 1.0)).z.abs() < 1e-6);
        assert!(((matrix * Vec4::new(0.0, 0.0, -101.0, 1.0)).z - 1.0).abs() < 1e-6);
        assert!(top_down_orthographic(0.0, 1.0, Vec2::ZERO, camera).is_err());
    }

    #[test]
    fn transparent_triangles_sort_by_centroid_depth() {
        let triangle = |z: f32| [ColorVertex { position: [0.0, 0.0, z], color: [1.0; 4] }; 3];
        let vertices: Vec<ColorVertex> = [triangle(5.0), triangle(-3.0), triangle(1.0)].concat();
        let order: Vec<f32> =
            sort_top_down_transparent_triangles(&vertices).chunks(3).map(|t| t[0].position[2]).collect();
        assert_eq!(order, [-3.0, 1.0, 5.0]);
    }
}
