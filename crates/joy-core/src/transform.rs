// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Entity and instance transforms.
//!
//! Euler angles are XYZ radians applied X, then Y, then Z, which is the matrix `Rz * Ry * Rx`. Transforms apply scale,
//! then rotation, then translation. The authored level format and physics adapter share these conventions.

use crate::math::{
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
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        rotation: Vec3::ZERO,
        scale: Vec3::ONE,
    };

    /// Create a transform from its three components.
    pub fn new(position: Vec3, rotation: Vec3, scale: Vec3) -> Self {
        Self {
            position,
            rotation,
            scale,
        }
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
        Self {
            position: self.transform_point(offset),
            rotation: self.rotation,
            scale: dimensions * self.scale,
        }
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
    let (x_squared, y_squared, z_squared) = (x.powi(2), y.powi(2), z.powi(2));

    let matrix11 = 1.0 - ((y_squared + z_squared) * 2.0);
    let matrix21 = 2.0 * ((x * y) + (z * w));
    let matrix31 = 2.0 * ((x * z) - (y * w));
    let cosine_y = matrix11.hypot(matrix21);
    let rotation_y = (-matrix31).atan2(cosine_y);

    if (cosine_y > GIMBAL_LOCK_EPSILON) {
        let rotation_x = (((y * z) + (x * w)) * 2.0).atan2(1.0 - ((x_squared + y_squared) * 2.0));
        let rotation_z = matrix21.atan2(matrix11);
        return Vec3::new(rotation_x, rotation_y, rotation_z);
    }

    let rotation_z = (((z * w) - (x * y)) * 2.0).atan2(1.0 - ((x_squared + z_squared) * 2.0));
    Vec3::new(0.0, rotation_y, rotation_z)
}

#[cfg(test)]
#[path = "transform.test.rs"]
mod tests;
