// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Conversions between engine math (glam 0.34) and the solver's math types, plus shared validation helpers.
//!
//! Rapier uses its own glam version, so values are copied component by component. Keeping every conversion here keeps
//! solver types out of the public API.
//!
//! Values that feed the simulation are computed with the solver's math, which `enhanced-determinism` backs with libm
//! and scalar (non-SIMD) arithmetic, so native and WebAssembly builds create bit-identical bodies. Values reported back
//! to callers use the engine's foundation helpers.

use joy_core::math::{
    Quat,
    Vec3,
};
use rapier3d::glamx::EulerRot;
use rapier3d::math::{
    ComplexField,
    Rotation,
    Vector,
};

use super::PhysicsError;

/// Copy an engine vector into the solver.
pub(super) fn to_solver_vector(value: Vec3) -> Vector {
    Vector::new(value.x, value.y, value.z)
}

/// Copy a solver vector into engine math.
pub(super) fn from_solver_vector(value: Vector) -> Vec3 {
    Vec3::new(value.x, value.y, value.z)
}

/// Build a solver rotation from XYZ Euler radians applied X, then Y, then Z (`Rz * Ry * Rx`).
///
/// Same convention as `joy_core::quat_from_euler`, evaluated with the solver's deterministic math.
pub(super) fn solver_rotation_from_euler(rotation: Vec3) -> Rotation {
    Rotation::from_euler(EulerRot::ZYX, rotation.z, rotation.y, rotation.x)
}

/// Deterministic `ln(1 + value)` for values that feed the simulation.
pub(super) fn deterministic_ln_1p(value: f64) -> f64 {
    ComplexField::ln_1p(value)
}

/// Decompose a solver rotation into XYZ Euler radians.
pub(super) fn euler_from_solver_rotation(rotation: Rotation) -> Vec3 {
    joy_core::euler_from_quat(Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w))
}

/// Validate a finite scalar against an inclusive range. Comparison happens in `f64` with the TypeScript bounds.
pub(super) fn finite_number(value: f32, name: &'static str, minimum: f64, maximum: f64) -> Result<f32, PhysicsError> {
    let widened = f64::from(value);
    if !widened.is_finite() || widened < minimum || widened > maximum {
        return Err(PhysicsError::OutOfRange { name, minimum, maximum });
    }
    Ok(value)
}

/// Validate that every component of a vector is finite.
pub(super) fn finite_vector(value: Vec3, name: &'static str) -> Result<Vec3, PhysicsError> {
    if !value.is_finite() {
        return Err(PhysicsError::NonFiniteVector(name));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "../tests/unit/convert.rs"]
mod tests;
