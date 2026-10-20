// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rigid body creation, queries, and mutation.
//!
//! Material and damping semantics follow cannon-es conventions:
//! - Friction and restitution of two touching bodies combine by multiplication.
//! - Damping is a per-second velocity fraction `d` in `[0, 1]`: cannon applies `v *= (1 - d)^dt`. Rapier applies
//!   `v *= 1 / (1 + dt * c)`, so each body uses `c = -ln(1 - d)`, which matches cannon to first order in `dt`. A
//!   damping of exactly one (cannon: velocity zeroed every step) maps to a large finite coefficient,
//!   `MAX_DAMPING_COEFFICIENT`.
//! - Bodies never sleep (cannon worlds default to `allowSleep = false`).
//! - Forces apply for exactly one fixed step; impulses change velocity immediately.

use joy_core::PrimitiveShape;
use joy_core::math::Vec3;
use rapier3d::dynamics::{
    CoefficientCombineRule,
    RigidBodyBuilder,
};
use rapier3d::geometry::{
    ColliderBuilder,
    SharedShape,
};
use rapier3d::math::{
    Pose,
    Vector,
};
use rapier3d::pipeline::ActiveHooks;

use super::convert::{
    deterministic_ln_1p,
    euler_from_solver_rotation,
    finite_number,
    finite_vector,
    from_solver_vector,
    solver_rotation_from_euler,
    to_solver_vector,
};
use super::physics_world::BodyRecord;
use super::sdf::SdfCollider;
use super::{
    BodyHandle,
    PhysicsError,
    PhysicsTransform,
    PhysicsWorld,
    SdfShape,
};

/// Smallest accepted sphere radius, compared in `f64`.
const MIN_SPHERE_RADIUS: f64 = f64::EPSILON;

/// Rapier damping coefficient used for a cannon damping of one; large enough to remove nearly all velocity per step.
const MAX_DAMPING_COEFFICIENT: f32 = 1.0e6;

/// Values a body assumes for omitted options. Lengths are meters, mass is kilograms, and damping is a per-second
/// velocity fraction in `[0, 1]`. The shape, position, and rotation are placeholders (a box at the origin) for use with
/// struct update syntax; see [`BodyOptions::new`].
pub const BODY_DEFAULTS: BodyOptions = BodyOptions {
    shape: PrimitiveShape::Box,
    mass: 0.0,
    half_extents: Vec3::new(0.5, 0.5, 0.5),
    radius: 0.5,
    position: Vec3::ZERO,
    rotation: Vec3::ZERO,
    friction: 0.5,
    restitution: 0.0,
    linear_damping: 0.01,
    angular_damping: 0.1,
};

/// Rigid body configuration. World coordinates are meters, Y up, +Z forward; rotation is XYZ Euler radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyOptions {
    /// Collider shape. Planes face +Y before rotation and must be static.
    pub shape: PrimitiveShape,
    /// Mass in kilograms; zero makes the body static (fixed). Nonnegative.
    pub mass: f32,
    /// Box half extents in meters; every component must be positive. Ignored for other shapes.
    pub half_extents: Vec3,
    /// Sphere radius in meters; must be positive. Ignored for other shapes.
    pub radius: f32,
    /// Initial world position in meters.
    pub position: Vec3,
    /// Initial XYZ Euler rotation in radians.
    pub rotation: Vec3,
    /// Friction coefficient; nonnegative. Combined with the other body's by multiplication.
    pub friction: f32,
    /// Restitution in `[0, 1]`. Combined with the other body's by multiplication.
    pub restitution: f32,
    /// Per-second linear velocity loss fraction in `[0, 1]` (cannon semantics).
    pub linear_damping: f32,
    /// Per-second angular velocity loss fraction in `[0, 1]` (cannon semantics).
    pub angular_damping: f32,
}

impl BodyOptions {
    /// Default options for a shape, at the origin with no rotation.
    pub const fn new(shape: PrimitiveShape) -> Self {
        Self {
            shape,
            ..BODY_DEFAULTS
        }
    }
}

impl Default for BodyOptions {
    fn default() -> Self {
        BODY_DEFAULTS
    }
}

impl PhysicsWorld {
    /// Create a world-owned rigid body and return its handle.
    ///
    /// Options are validated in a fixed order (mass, friction, restitution, position, rotation, damping, shape
    /// dimensions, static planes), so the first failure reported is stable; invalid configuration leaves the world
    /// unchanged.
    pub fn create_body(&mut self, options: BodyOptions) -> Result<BodyHandle, PhysicsError> {
        let validated = ValidatedBody::new(&options)?;
        let shape = create_shape(&options)?;
        if options.shape == PrimitiveShape::Plane && validated.mass != 0.0 {
            return Err(PhysicsError::DynamicPlane);
        }
        Ok(self.insert_body(validated, shape, ActiveHooks::empty()))
    }

    /// Create a world-owned rigid body whose collider is a signed distance shape, and return its handle.
    ///
    /// The shape replaces the primitive collider: `options.shape`, `half_extents`, and `radius` are ignored, while mass,
    /// pose, friction, restitution, and damping are validated and applied exactly as in [`PhysicsWorld::create_body`].
    /// The shape's local frame is the body frame, and its center of mass and inertia come from the field. The body
    /// shares `shape`'s data; dropping the last body and handle releases it. Invalid options leave the world unchanged.
    pub fn create_sdf_body(&mut self, shape: &SdfShape, options: BodyOptions) -> Result<BodyHandle, PhysicsError> {
        let validated = ValidatedBody::new(&options)?;

        // Rapier recycles a pair's previous contacts while its relative pose barely moves, but SDF manifolds are sparse
        // and refine themselves step by step (each new sample can displace a worse cached point), so stale ones let
        // bodies tip. Pairs with active hooks are never recycled; the world's default hooks keep every solver flag.
        let collider = SharedShape::new(SdfCollider {
            shape: shape.clone(),
        });
        Ok(self.insert_body(validated, collider, ActiveHooks::FILTER_CONTACT_PAIRS))
    }

    /// Remove a body, including the vehicle it carries when it is a vehicle chassis. Returns whether a live body was
    /// removed.
    pub fn remove_body(&mut self, handle: BodyHandle) -> bool {
        let Some(raw) = self.resolve_body(handle) else {
            return false;
        };
        self.bodies.shift_remove(&raw);
        self.forced_bodies.retain(|forced| *forced != raw);
        self.solver.remove_body(raw);
        true
    }

    /// Whether `handle` names a live body owned by this world.
    pub fn contains_body(&self, handle: BodyHandle) -> bool {
        self.resolve_body(handle).is_some()
    }

    /// Copied world pose of a body, or `None` for an unknown handle.
    pub fn body_transform(&self, handle: BodyHandle) -> Option<PhysicsTransform> {
        let body = self.solver.bodies.get(self.resolve_body(handle)?)?;
        Some(PhysicsTransform {
            position: from_solver_vector(body.translation()),
            rotation: euler_from_solver_rotation(*body.rotation()),
        })
    }

    /// Copied world pose for presentation: the body's pose before the latest fixed step blended toward its current
    /// pose by [`interpolation`](Self::interpolation), or `None` for an unknown handle.
    ///
    /// Positions blend linearly and rotations along the shortest arc. Use it for rendering and cameras only; gameplay
    /// reads [`body_transform`](Self::body_transform), the solver's state.
    pub fn interpolated_body_transform(&self, handle: BodyHandle) -> Option<PhysicsTransform> {
        let pose = self.interpolated_pose(handle)?;
        Some(PhysicsTransform {
            position: from_solver_vector(pose.translation),
            rotation: euler_from_solver_rotation(pose.rotation),
        })
    }

    /// Teleport a body in world space without changing its velocity, and wake it.
    ///
    /// Omitted components keep their current value. Non-finite input fails before any state changes.
    pub fn set_body_transform(
        &mut self,
        handle: BodyHandle,
        position: Option<Vec3>,
        rotation: Option<Vec3>,
    ) -> Result<(), PhysicsError> {
        let position = position.map(|value| finite_vector(value, "position")).transpose()?;
        let rotation = rotation.map(|value| finite_vector(value, "rotation")).transpose()?;
        let raw = self.resolve_body(handle).ok_or(PhysicsError::UnknownBody)?;
        let body = self.solver.bodies.get_mut(raw).ok_or(PhysicsError::UnknownBody)?;

        let current = *body.position();
        let translation = position.map(to_solver_vector).unwrap_or(current.translation);
        let orientation = rotation.map(solver_rotation_from_euler).unwrap_or(current.rotation);
        let pose = Pose::from_parts(translation, orientation);
        body.set_position(pose, true);

        // A teleport is a cut: presentation starts from the new pose instead of sweeping across the jump.
        if let Some(record) = self.bodies.get_mut(&raw) {
            record.previous_pose = pose;
        }
        Ok(())
    }

    /// Copied world-space linear velocity in meters per second, or `None` for an unknown handle.
    pub fn body_velocity(&self, handle: BodyHandle) -> Option<Vec3> {
        let body = self.solver.bodies.get(self.resolve_body(handle)?)?;
        Some(from_solver_vector(body.linvel()))
    }

    /// Set world-space linear velocity in meters per second and wake the body. Static bodies ignore velocity.
    pub fn set_body_velocity(&mut self, handle: BodyHandle, velocity: Vec3) -> Result<(), PhysicsError> {
        let velocity = finite_vector(velocity, "velocity")?;
        let raw = self.resolve_body(handle).ok_or(PhysicsError::UnknownBody)?;
        let body = self.solver.bodies.get_mut(raw).ok_or(PhysicsError::UnknownBody)?;
        body.set_linvel(to_solver_vector(velocity), true);
        Ok(())
    }

    /// Apply a force in newtons for the next fixed step.
    ///
    /// `point` is a world-space application point; `None` applies at the center of mass (no torque). Forces accumulate
    /// until a fixed step consumes them, then reset, like cannon-es. Static bodies ignore forces.
    pub fn apply_force(&mut self, handle: BodyHandle, force: Vec3, point: Option<Vec3>) -> Result<(), PhysicsError> {
        let force = finite_vector(force, "force")?;
        let point = point.map(|value| finite_vector(value, "point")).transpose()?;
        let raw = self.resolve_body(handle).ok_or(PhysicsError::UnknownBody)?;
        let body = self.solver.bodies.get_mut(raw).ok_or(PhysicsError::UnknownBody)?;
        match point {
            Some(point) => body.add_force_at_point(to_solver_vector(force), to_solver_vector(point), true),
            None => body.add_force(to_solver_vector(force), true),
        }
        if !self.forced_bodies.contains(&raw) {
            self.forced_bodies.push(raw);
        }
        Ok(())
    }

    /// Apply an instantaneous impulse in newton-seconds; velocity changes immediately.
    ///
    /// `point` is a world-space application point; `None` applies at the center of mass. Static bodies ignore impulses.
    pub fn apply_impulse(
        &mut self,
        handle: BodyHandle,
        impulse: Vec3,
        point: Option<Vec3>,
    ) -> Result<(), PhysicsError> {
        let impulse = finite_vector(impulse, "impulse")?;
        let point = point.map(|value| finite_vector(value, "point")).transpose()?;
        let raw = self.resolve_body(handle).ok_or(PhysicsError::UnknownBody)?;
        let body = self.solver.bodies.get_mut(raw).ok_or(PhysicsError::UnknownBody)?;
        match point {
            Some(point) => body.apply_impulse_at_point(to_solver_vector(impulse), to_solver_vector(point), true),
            None => body.apply_impulse(to_solver_vector(impulse), true),
        }
        Ok(())
    }

    /// Body mass in kilograms (zero for static bodies), or `None` for an unknown handle.
    pub fn body_mass(&self, handle: BodyHandle) -> Option<f32> {
        let raw = self.resolve_body(handle)?;
        let record = self.bodies.get(&raw)?;
        if !record.dynamic {
            return Some(0.0);
        }
        Some(self.solver.bodies.get(raw)?.mass())
    }
}

impl PhysicsWorld {
    /// The presentation pose of a live body: its previous fixed-step pose blended toward the current one.
    pub(super) fn interpolated_pose(&self, handle: BodyHandle) -> Option<Pose> {
        let raw = self.resolve_body(handle)?;
        let record = self.bodies.get(&raw)?;
        let current = *self.solver.bodies.get(raw)?.position();
        let alpha = self.interpolation();
        let previous = record.previous_pose;
        Some(Pose::from_parts(
            previous.translation.lerp(current.translation, alpha),
            previous.rotation.slerp(current.rotation, alpha),
        ))
    }

    /// Insert a validated body with its single collider; static bodies (mass zero) are fixed, exactly like cannon-es.
    pub(super) fn insert_body(
        &mut self,
        validated: ValidatedBody,
        shape: SharedShape,
        hooks: ActiveHooks,
    ) -> BodyHandle {
        let dynamic = validated.mass > 0.0;
        let builder = if dynamic { RigidBodyBuilder::dynamic() } else { RigidBodyBuilder::fixed() };
        let pose =
            Pose::from_parts(to_solver_vector(validated.position), solver_rotation_from_euler(validated.rotation));
        let body = builder
            .pose(pose)
            .linear_damping(damping_coefficient(validated.linear_damping))
            .angular_damping(damping_coefficient(validated.angular_damping))
            .can_sleep(false)
            .build();

        let collider = ColliderBuilder::new(shape)
            .friction(validated.friction)
            .friction_combine_rule(CoefficientCombineRule::Multiply)
            .restitution(validated.restitution)
            .restitution_combine_rule(CoefficientCombineRule::Multiply)
            .active_hooks(hooks);
        let collider = if dynamic { collider.mass(validated.mass) } else { collider.density(0.0) };

        let raw = self.solver.bodies.insert(body);
        let collider = self.solver.colliders.insert_with_parent(collider.build(), raw, &mut self.solver.bodies);
        if let Some(body) = self.solver.bodies.get_mut(raw) {
            // Mass is queried (and used by vehicles) before the first step, so resolve it immediately.
            body.recompute_mass_properties_from_colliders(&self.solver.colliders);
        }
        self.bodies.insert(
            raw,
            BodyRecord {
                collider,
                dynamic,
                previous_pose: pose,
                vehicle: None,
            },
        );
        BodyHandle {
            world: self.world_id,
            raw,
        }
    }
}

/// Body options that passed validation, independent of the collider shape.
pub(super) struct ValidatedBody {
    pub(super) mass: f32,
    friction: f32,
    restitution: f32,
    position: Vec3,
    rotation: Vec3,
    linear_damping: f32,
    angular_damping: f32,
}

impl ValidatedBody {
    /// Validate in a fixed order: mass, friction, restitution, position, rotation, then damping.
    pub(super) fn new(options: &BodyOptions) -> Result<Self, PhysicsError> {
        Ok(Self {
            mass: finite_number(options.mass, "mass", 0.0, f64::INFINITY)?,
            friction: finite_number(options.friction, "friction", 0.0, f64::INFINITY)?,
            restitution: finite_number(options.restitution, "restitution", 0.0, 1.0)?,
            position: finite_vector(options.position, "position")?,
            rotation: finite_vector(options.rotation, "rotation")?,
            linear_damping: finite_number(options.linear_damping, "linearDamping", 0.0, 1.0)?,
            angular_damping: finite_number(options.angular_damping, "angularDamping", 0.0, 1.0)?,
        })
    }
}

/// Validate collider dimensions and build the solver shape.
pub(super) fn create_shape(options: &BodyOptions) -> Result<SharedShape, PhysicsError> {
    match options.shape {
        PrimitiveShape::Box => {
            let half_extents = finite_vector(options.half_extents, "halfExtents")?;
            if half_extents.min_element() <= 0.0 {
                return Err(PhysicsError::NonPositiveHalfExtents);
            }
            Ok(SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z))
        }
        PrimitiveShape::Sphere => {
            let radius = finite_number(options.radius, "radius", MIN_SPHERE_RADIUS, f64::INFINITY)?;
            Ok(SharedShape::ball(radius))
        }
        // Authored planes face +Y before rotation, which is the half-space's outward normal.
        PrimitiveShape::Plane => Ok(SharedShape::halfspace(Vector::Y)),
    }
}

/// Convert a cannon per-second damping fraction into rapier's damping coefficient (`c = -ln(1 - d)`).
fn damping_coefficient(damping: f32) -> f32 {
    if damping >= 1.0 {
        return MAX_DAMPING_COEFFICIENT;
    }
    let coefficient = -deterministic_ln_1p(-f64::from(damping));
    (coefficient as f32).min(MAX_DAMPING_COEFFICIENT)
}

#[cfg(test)]
#[path = "physics_body.test.rs"]
mod tests;
