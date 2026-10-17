// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Entity and instance transforms.
//!
//! Euler angles are XYZ radians applied X, then Y, then Z, which is the matrix `Rz * Ry * Rx`. Transforms apply scale,
//! then rotation, then translation. The authored level format and physics adapter share these conventions.

use glam::{
    EulerRot,
    Mat4,
    Quat,
    Vec3,
};
use serde::{
    Deserialize,
    Serialize,
};

/// Below this cosine the Euler decomposition treats the rotation as gimbal locked.
const GIMBAL_LOCK_EPSILON: f32 = 1e-6;

/// Position, XYZ Euler rotation, and scale in world (or parent) space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Translation in world units.
    pub position: Vec3,
    /// XYZ Euler radians applied X, then Y, then Z.
    pub rotation: Vec3,
    /// Per-axis scale applied before rotation.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// No translation, no rotation, and unit scale.
    pub const IDENTITY: Self = Self { position: Vec3::ZERO, rotation: Vec3::ZERO, scale: Vec3::ONE };

    /// Create a transform from its three components.
    pub fn new(position: Vec3, rotation: Vec3, scale: Vec3) -> Self {
        Self { position, rotation, scale }
    }

    /// Rotation as a quaternion using the engine's Euler convention.
    pub fn rotation_quat(&self) -> Quat {
        quat_from_euler(self.rotation)
    }

    /// Column-major affine matrix: scale, rotate X/Y/Z, then translate.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation_quat(), self.position)
    }

    /// Transform a local-space point into this transform's space.
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        self.position + (self.rotation_quat() * (point * self.scale))
    }

    /// Place a child offset in this transform's frame, returning a new transform that shares this rotation.
    ///
    /// `offset` is in local units and is scaled by this transform; `dimensions` is the child's full local size and is
    /// also scaled, so attached details follow the parent's pitch, roll, and scale.
    pub fn attach(&self, offset: Vec3, dimensions: Vec3) -> Self {
        Self { position: self.transform_point(offset), rotation: self.rotation, scale: dimensions * self.scale }
    }
}

/// Build a quaternion from XYZ Euler radians applied X, then Y, then Z.
pub fn quat_from_euler(rotation: Vec3) -> Quat {
    // glam's intrinsic ZYX order composes `Rz * Ry * Rx`, which applies X first in world axes.
    Quat::from_euler(EulerRot::ZYX, rotation.z, rotation.y, rotation.x)
}

/// Decompose a rotation into XYZ Euler radians applied X, then Y, then Z.
///
/// At gimbal lock the equivalent canonical rotation fixes X to zero, matching the TypeScript physics adapter.
pub fn euler_from_quat(rotation: Quat) -> Vec3 {
    let (x, y, z, w) = (rotation.x, rotation.y, rotation.z, rotation.w);
    let matrix11 = 1.0 - (2.0 * ((y * y) + (z * z)));
    let matrix21 = 2.0 * ((x * y) + (z * w));
    let matrix31 = 2.0 * ((x * z) - (y * w));
    let cosine_y = matrix11.hypot(matrix21);
    let rotation_y = (-matrix31).atan2(cosine_y);

    if cosine_y > GIMBAL_LOCK_EPSILON {
        let rotation_x = (2.0 * ((y * z) + (x * w))).atan2(1.0 - (2.0 * ((x * x) + (y * y))));
        let rotation_z = matrix21.atan2(matrix11);
        return Vec3::new(rotation_x, rotation_y, rotation_z);
    }

    let rotation_z = (2.0 * ((z * w) - (x * y))).atan2(1.0 - (2.0 * ((x * x) + (z * z))));
    Vec3::new(0.0, rotation_y, rotation_z)
}

#[cfg(test)]
mod tests {
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
        let parent =
            Transform::new(Vec3::new(5.0, 0.0, 0.0), Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0), Vec3::ONE);
        let child = parent.attach(Vec3::new(0.0, 0.0, 1.0), Vec3::splat(0.5));
        assert!(close(child.position, Vec3::new(6.0, 0.0, 0.0)));
        assert!(close(child.scale, Vec3::splat(0.5)));
    }
}
