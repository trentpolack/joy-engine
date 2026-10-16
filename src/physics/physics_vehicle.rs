// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Raycast vehicles: data-defined wheel arrays with ray suspension, drive, steering, and brakes.
//!
//! Vehicles use rapier's `DynamicRayCastVehicleController`, which, like cannon-es's `RaycastVehicle`, is a port of
//! Bullet's `btRaycastVehicle`. Chassis axes are up = +Y, forward = +Z; every wheel casts along chassis -Y and spins
//! around the chassis -X axle. Positive throttle drives local +Z,
//! positive steer turns toward local +X, and brake is an impulse bound scaled by `brake_force` on braking wheels.
//!
//! Parameter mapping (authored JSON name → rapier wheel field): `suspensionStiffness` → `suspension_stiffness`,
//! `dampingCompression` → `damping_compression`, `dampingRelaxation` → `damping_relaxation`,
//! `maxSuspensionTravel` → `max_suspension_travel`, `maxSuspensionForce` → `max_suspension_force`, and
//! `frictionSlip` → `friction_slip`; side friction stiffness is 1 like cannon. Known solver differences:
//! - `rollInfluence` is validated and stored but rapier 0.36 does not expose it; rapier applies a fixed 0.1 (the
//!   authored default is 0.01), so side impulses act slightly further from the center of mass than in cannon.
//! - Rapier divides the rolling/brake friction impulse by the number of grounded wheels before clamping; cannon does
//!   not. Brake impulses that hit the `brake_force` bound behave identically; gentle rolling resistance is weaker.
//! - Rapier does not apply the equal-and-opposite side impulse to a dynamic ground body.

use glam::Vec3;
use rapier3d::control::{DynamicRayCastVehicleController, WheelTuning};
use rapier3d::math::Rotation;

use super::convert::{euler_from_solver_rotation, finite_number, finite_vector, from_solver_vector, to_solver_vector};
use super::physics_world::VehicleRecord;
use super::{BodyHandle, PhysicsError, PhysicsTransform, PhysicsWorld, VehicleHandle};

// ---------------------------------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------------------------------

/// Chassis-space up axis index (Y).
const UP_AXIS_INDEX: usize = 1;

/// Chassis-space forward axis index (Z).
const FORWARD_AXIS_INDEX: usize = 2;

/// Chassis-space suspension ray direction.
const WHEEL_DIRECTION: Vec3 = Vec3::new(0.0, -1.0, 0.0);

/// Chassis-space wheel axle.
const WHEEL_AXLE: Vec3 = Vec3::new(-1.0, 0.0, 0.0);

/// Cannon's side-friction stiffness (`sideFrictionStiffness2`).
const SIDE_FRICTION_STIFFNESS: f32 = 1.0;

/// Largest accepted steering angle in radians.
const MAX_STEER_LIMIT: f64 = std::f64::consts::FRAC_PI_2;

/// Smallest accepted wheel radius (JavaScript `Number.EPSILON`), compared in `f64`.
const MIN_WHEEL_RADIUS: f64 = f64::EPSILON;

/// Values a vehicle assumes for omitted options, matching the TypeScript `PHYSICS_VEHICLE_DEFAULTS`. Forces are
/// newtons and steering is radians. Wheel `friction_slip` defaults to the vehicle's own value.
pub const VEHICLE_DEFAULTS: VehicleOptions =
    VehicleOptions { wheels: Vec::new(), engine_force: 400.0, max_steer: 0.45, brake_force: 20.0, friction_slip: 5.0 };

/// Values a wheel assumes for omitted options, matching the TypeScript `PHYSICS_WHEEL_DEFAULTS`. Lengths are
/// chassis-local meters. The position is a placeholder (the chassis origin); see [`WheelOptions::new`].
pub const WHEEL_DEFAULTS: WheelOptions = WheelOptions {
    position: Vec3::ZERO,
    radius: 0.35,
    steering: false,
    driven: false,
    brake: false,
    suspension_rest_length: 0.3,
    suspension_stiffness: 30.0,
    damping_compression: 4.4,
    damping_relaxation: 2.3,
    max_suspension_travel: 0.3,
    max_suspension_force: 100_000.0,
    friction_slip: None,
    roll_influence: 0.01,
};

// ---------------------------------------------------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------------------------------------------------

/// One raycast wheel, in chassis-local meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelOptions {
    /// Suspension attachment point in chassis space (meters).
    pub position: Vec3,
    /// Tire radius in meters; positive.
    pub radius: f32,
    /// Whether steering input turns this wheel.
    pub steering: bool,
    /// Whether throttle drives this wheel.
    pub driven: bool,
    /// Whether brake input slows this wheel.
    pub brake: bool,
    /// Suspension rest length in meters; nonnegative.
    pub suspension_rest_length: f32,
    /// Spring stiffness (per unit chassis mass); nonnegative.
    pub suspension_stiffness: f32,
    /// Damping while compressing; nonnegative.
    pub damping_compression: f32,
    /// Damping while extending; nonnegative.
    pub damping_relaxation: f32,
    /// Travel allowed above and below the rest length, in meters; nonnegative.
    pub max_suspension_travel: f32,
    /// Suspension force bound in newtons; nonnegative.
    pub max_suspension_force: f32,
    /// Tire grip; `None` uses the vehicle's `friction_slip`. Nonnegative.
    pub friction_slip: Option<f32>,
    /// Height fraction at which side impulses apply, in `[0, 1]`. Validated and stored; see the module notes.
    pub roll_influence: f32,
}

impl WheelOptions {
    /// Default wheel tuning at a chassis-space position.
    pub const fn new(position: Vec3) -> Self {
        Self { position, ..WHEEL_DEFAULTS }
    }
}

impl Default for WheelOptions {
    fn default() -> Self {
        WHEEL_DEFAULTS
    }
}

/// Vehicle configuration. Forces are newtons and steering is radians.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleOptions {
    /// Wheels in chassis space; at least one is required.
    pub wheels: Vec<WheelOptions>,
    /// Drive force per driven wheel at full throttle, in newtons; nonnegative.
    pub engine_force: f32,
    /// Steering angle at full steer, in radians, in `[0, π/2]`.
    pub max_steer: f32,
    /// Brake impulse bound per braking wheel at full brake; nonnegative.
    pub brake_force: f32,
    /// Default tire grip for wheels without their own value; nonnegative.
    pub friction_slip: f32,
}

impl Default for VehicleOptions {
    fn default() -> Self {
        VEHICLE_DEFAULTS
    }
}

/// Drive limits a running vehicle can retune, with the same bounds as [`VehicleOptions`]. Matches the writable
/// `engineForce`, `maxSteer`, and `brakeForce` properties of the TypeScript `PhysicsVehicle`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VehicleTuning {
    /// Drive force per driven wheel at full throttle, in newtons; nonnegative.
    pub engine_force: f32,
    /// Steering angle at full steer, in radians, in `[0, π/2]`.
    pub max_steer: f32,
    /// Brake impulse bound per braking wheel at full brake; nonnegative.
    pub brake_force: f32,
}

/// Driver intent. Values are clamped (`throttle`/`steer` to `[-1, 1]`, `brake` to `[0, 1]`); the default is all zero,
/// so omitted fields mean "released".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VehicleInput {
    /// Drive along local +Z (positive) or -Z (negative).
    pub throttle: f32,
    /// Turn toward local +X (positive) or -X (negative).
    pub steer: f32,
    /// Brake amount.
    pub brake: f32,
}

// ---------------------------------------------------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------------------------------------------------

impl PhysicsWorld {
    /// Attach a raycast vehicle to an owned dynamic chassis. The vehicle borrows its chassis.
    ///
    /// Fails if the chassis is static, unknown, foreign, or already carries a vehicle, or if any option is invalid;
    /// every wheel is validated before the world changes.
    pub fn create_vehicle(
        &mut self,
        chassis: BodyHandle,
        options: VehicleOptions,
    ) -> Result<VehicleHandle, PhysicsError> {
        let raw = self.resolve_body(chassis).ok_or(PhysicsError::InvalidChassis)?;
        let record = self.bodies.get(&raw).ok_or(PhysicsError::InvalidChassis)?;
        if !record.dynamic || record.vehicle.is_some() {
            return Err(PhysicsError::InvalidChassis);
        }

        let engine_force = finite_number(options.engine_force, "engineForce", 0.0, f64::INFINITY)?;
        let max_steer = finite_number(options.max_steer, "maxSteer", 0.0, MAX_STEER_LIMIT)?;
        let brake_force = finite_number(options.brake_force, "brakeForce", 0.0, f64::INFINITY)?;
        let friction_slip = finite_number(options.friction_slip, "frictionSlip", 0.0, f64::INFINITY)?;
        if options.wheels.is_empty() {
            return Err(PhysicsError::NoWheels);
        }
        let wheels =
            options.wheels.iter().map(|wheel| validate_wheel(wheel, friction_slip)).collect::<Result<Vec<_>, _>>()?;

        let mut controller = DynamicRayCastVehicleController::new(raw);
        controller.index_up_axis = UP_AXIS_INDEX;
        controller.index_forward_axis = FORWARD_AXIS_INDEX;
        for wheel in &wheels {
            let tuning = WheelTuning {
                suspension_stiffness: wheel.suspension_stiffness,
                suspension_compression: wheel.damping_compression,
                suspension_damping: wheel.damping_relaxation,
                max_suspension_travel: wheel.max_suspension_travel,
                side_friction_stiffness: SIDE_FRICTION_STIFFNESS,
                friction_slip: wheel.friction_slip.unwrap_or(friction_slip),
                max_suspension_force: wheel.max_suspension_force,
            };
            controller.add_wheel(
                to_solver_vector(wheel.position),
                to_solver_vector(WHEEL_DIRECTION),
                to_solver_vector(WHEEL_AXLE),
                wheel.suspension_rest_length,
                wheel.radius,
                &tuning,
            );
        }

        let id = self.next_vehicle_id;
        self.next_vehicle_id += 1;
        self.vehicles
            .insert(id, VehicleRecord { controller, chassis: raw, wheels, engine_force, max_steer, brake_force });
        if let Some(record) = self.bodies.get_mut(&raw) {
            record.vehicle = Some(id);
        }
        Ok(VehicleHandle { world: self.world_id, id })
    }

    /// Replace driver intent for every wheel and keep the chassis awake.
    ///
    /// Non-finite values fail without changing the vehicle. Driven wheels receive `throttle * engine_force`, steering
    /// wheels `steer * max_steer`, and braking wheels a brake impulse bound of `brake * brake_force`.
    pub fn set_vehicle_input(&mut self, handle: VehicleHandle, input: VehicleInput) -> Result<(), PhysicsError> {
        let throttle = finite_number(input.throttle, "throttle", f64::NEG_INFINITY, f64::INFINITY)?.clamp(-1.0, 1.0);
        let steer = finite_number(input.steer, "steer", f64::NEG_INFINITY, f64::INFINITY)?.clamp(-1.0, 1.0);
        let brake = finite_number(input.brake, "brake", f64::NEG_INFINITY, f64::INFINITY)?.clamp(0.0, 1.0);
        let record = self.resolve_vehicle_mut(handle).ok_or(PhysicsError::UnknownVehicle)?;

        let (engine_force, max_steer, brake_force) = (record.engine_force, record.max_steer, record.brake_force);
        for (wheel, options) in record.controller.wheels_mut().iter_mut().zip(&record.wheels) {
            // Rapier's tire forward tangent is +Z for this axle, so positive throttle drives forward directly
            // (cannon's tangent was -Z, which is why the TypeScript adapter negated it).
            wheel.engine_force = if options.driven { throttle * engine_force } else { 0.0 };
            wheel.steering = if options.steering { steer * max_steer } else { 0.0 };
            wheel.brake = if options.brake { brake * brake_force } else { 0.0 };
        }

        let chassis = record.chassis;
        if let Some(body) = self.solver.bodies.get_mut(chassis) {
            body.wake_up(true);
        }
        Ok(())
    }

    /// The vehicle's current drive limits, or `None` for an unknown handle.
    pub fn vehicle_tuning(&self, handle: VehicleHandle) -> Option<VehicleTuning> {
        self.resolve_vehicle(handle).map(|record| VehicleTuning {
            engine_force: record.engine_force,
            max_steer: record.max_steer,
            brake_force: record.brake_force,
        })
    }

    /// Replace the vehicle's drive limits; they apply from the next [`set_vehicle_input`](Self::set_vehicle_input).
    /// Out-of-range values fail without changing the vehicle.
    pub fn set_vehicle_tuning(&mut self, handle: VehicleHandle, tuning: VehicleTuning) -> Result<(), PhysicsError> {
        let engine_force = finite_number(tuning.engine_force, "engineForce", 0.0, f64::INFINITY)?;
        let max_steer = finite_number(tuning.max_steer, "maxSteer", 0.0, MAX_STEER_LIMIT)?;
        let brake_force = finite_number(tuning.brake_force, "brakeForce", 0.0, f64::INFINITY)?;
        let record = self.resolve_vehicle_mut(handle).ok_or(PhysicsError::UnknownVehicle)?;
        record.engine_force = engine_force;
        record.max_steer = max_steer;
        record.brake_force = brake_force;
        Ok(())
    }

    /// Copied wheel poses in world meters and XYZ Euler radians, in wheel order; empty for an unknown handle.
    ///
    /// Each pose is the suspension hard point pushed down by the current suspension length, oriented by the chassis,
    /// the wheel's steering angle around chassis up, and its spin around the axle (positive spin rolls forward).
    pub fn wheel_transforms(&self, handle: VehicleHandle) -> Vec<PhysicsTransform> {
        let Some(record) = self.resolve_vehicle(handle) else {
            return Vec::new();
        };
        let Some(chassis) = self.solver.bodies.get(record.chassis) else {
            return Vec::new();
        };
        let pose = *chassis.position();
        record
            .controller
            .wheels()
            .iter()
            .map(|wheel| {
                let hard_point = pose.transform_point(wheel.chassis_connection_point_cs);
                let direction = pose.rotation * wheel.direction_cs;
                let position = hard_point + (direction * wheel.raycast_info().suspension_length);

                // Cannon composes chassis * steer(up) * spin(axle). Rapier accumulates spin with the opposite sign
                // for this axle, so it is negated to keep wheels visibly rolling forward like the TypeScript engine.
                let up = -wheel.direction_cs.normalize_or_zero();
                let axle = wheel.axle_cs.normalize_or_zero();
                let steer = Rotation::from_axis_angle(up, wheel.steering);
                let spin = Rotation::from_axis_angle(axle, -wheel.rotation);
                let orientation = (pose.rotation * steer * spin).normalize();
                PhysicsTransform {
                    position: from_solver_vector(position),
                    rotation: euler_from_solver_rotation(orientation),
                }
            })
            .collect()
    }

    /// Unsigned chassis speed in meters per second, or zero for an unknown handle.
    pub fn vehicle_speed(&self, handle: VehicleHandle) -> f32 {
        self.resolve_vehicle(handle)
            .and_then(|record| self.solver.bodies.get(record.chassis))
            .map_or(0.0, |body| body.linvel().length())
    }

    /// Wheels whose suspension ray touched ground during the most recent fixed step; zero for an unknown handle.
    pub fn grounded_wheel_count(&self, handle: VehicleHandle) -> usize {
        self.resolve_vehicle(handle).map_or(0, |record| {
            record.controller.wheels().iter().filter(|wheel| wheel.raycast_info().is_in_contact).count()
        })
    }

    /// Detach and remove a vehicle while keeping its chassis body. Returns whether a live vehicle was removed.
    pub fn remove_vehicle(&mut self, handle: VehicleHandle) -> bool {
        if handle.world != self.world_id {
            return false;
        }
        let Some(record) = self.vehicles.shift_remove(&handle.id) else {
            return false;
        };
        if let Some(body) = self.bodies.get_mut(&record.chassis) {
            body.vehicle = None;
        }
        true
    }

    /// Whether `handle` names a live vehicle owned by this world.
    pub fn contains_vehicle(&self, handle: VehicleHandle) -> bool {
        self.resolve_vehicle(handle).is_some()
    }

    /// The vehicle attached to a body, if any.
    pub fn body_vehicle(&self, handle: BodyHandle) -> Option<VehicleHandle> {
        let raw = self.resolve_body(handle)?;
        let id = self.bodies.get(&raw)?.vehicle?;
        Some(VehicleHandle { world: self.world_id, id })
    }

    /// The chassis body a vehicle borrows, or `None` for an unknown handle.
    pub fn vehicle_chassis(&self, handle: VehicleHandle) -> Option<BodyHandle> {
        let record = self.resolve_vehicle(handle)?;
        Some(BodyHandle { world: self.world_id, raw: record.chassis })
    }

    /// Resolve a public vehicle handle owned by this world.
    fn resolve_vehicle(&self, handle: VehicleHandle) -> Option<&VehicleRecord> {
        if handle.world != self.world_id {
            return None;
        }
        self.vehicles.get(&handle.id)
    }

    /// Resolve a public vehicle handle owned by this world for mutation.
    fn resolve_vehicle_mut(&mut self, handle: VehicleHandle) -> Option<&mut VehicleRecord> {
        if handle.world != self.world_id {
            return None;
        }
        self.vehicles.get_mut(&handle.id)
    }
}

/// Validate one wheel and resolve its friction slip.
fn validate_wheel(wheel: &WheelOptions, friction_slip: f32) -> Result<WheelOptions, PhysicsError> {
    Ok(WheelOptions {
        position: finite_vector(wheel.position, "wheel.position")?,
        radius: finite_number(wheel.radius, "wheel.radius", MIN_WHEEL_RADIUS, f64::INFINITY)?,
        steering: wheel.steering,
        driven: wheel.driven,
        brake: wheel.brake,
        suspension_rest_length: finite_number(
            wheel.suspension_rest_length,
            "wheel.suspensionRestLength",
            0.0,
            f64::INFINITY,
        )?,
        suspension_stiffness: finite_number(
            wheel.suspension_stiffness,
            "wheel.suspensionStiffness",
            0.0,
            f64::INFINITY,
        )?,
        damping_compression: finite_number(wheel.damping_compression, "wheel.dampingCompression", 0.0, f64::INFINITY)?,
        damping_relaxation: finite_number(wheel.damping_relaxation, "wheel.dampingRelaxation", 0.0, f64::INFINITY)?,
        max_suspension_travel: finite_number(
            wheel.max_suspension_travel,
            "wheel.maxSuspensionTravel",
            0.0,
            f64::INFINITY,
        )?,
        max_suspension_force: finite_number(
            wheel.max_suspension_force,
            "wheel.maxSuspensionForce",
            0.0,
            f64::INFINITY,
        )?,
        friction_slip: Some(finite_number(
            wheel.friction_slip.unwrap_or(friction_slip),
            "wheel.frictionSlip",
            0.0,
            f64::INFINITY,
        )?),
        roll_influence: finite_number(wheel.roll_influence, "wheel.rollInfluence", 0.0, 1.0)?,
    })
}
