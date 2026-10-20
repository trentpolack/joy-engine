// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The physics world owner: fixed-step advancement with bounded catch-up, body bookkeeping (including the vehicles
//! some bodies carry), and raycasts.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use indexmap::IndexMap;
use joy_core::math::Vec3;
use rapier3d::control::DynamicRayCastVehicleController;
use rapier3d::dynamics::RigidBodyHandle;
use rapier3d::geometry::{
    ColliderHandle,
    NarrowPhase,
    Ray,
};
use rapier3d::math::Pose;
use rapier3d::parry::query::{
    DefaultQueryDispatcher,
    QueryDispatcher,
};
use rapier3d::pipeline::QueryFilter;

use super::convert::{
    finite_number,
    finite_vector,
    from_solver_vector,
    to_solver_vector,
};
use super::sdf::{
    SdfCollisionCounters,
    SdfQueryDispatcher,
};
use super::{
    BodyHandle,
    PhysicsError,
    RaycastHit,
    SdfCollisionStatistics,
    WheelOptions,
};

// ---------------------------------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------------------------------

/// Default gravity in meters per second squared (Y-up).
const DEFAULT_GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

/// Default fixed step in seconds.
const DEFAULT_FIXED_STEP: f32 = 1.0 / 60.0;

/// Default bound on fixed steps per `step` call.
const DEFAULT_MAX_SUB_STEPS: u32 = 5;

/// Smallest accepted fixed step in seconds.
const MIN_FIXED_STEP: f64 = 1.0 / 1000.0;

/// Largest accepted fixed step in seconds.
const MAX_FIXED_STEP: f64 = 1.0;

/// Largest accepted sub-step bound.
const MAX_SUB_STEPS_LIMIT: u32 = 120;

/// Tolerance that lets an accumulator a hair below one fixed step still execute it, so rounding cannot drop a step.
const ACCUMULATOR_EPSILON: f64 = 1e-12;

/// Source of world identities. This is identity bookkeeping only (it lets handles detect foreign worlds); it holds no
/// simulation state and never influences simulation results.
static NEXT_WORLD_ID: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------------------------------------------------

/// Solver policy. Gravity is meters per second squared; the fixed step is seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsWorldOptions {
    /// World gravity in meters per second squared; default `[0, -9.81, 0]`.
    pub gravity: Vec3,
    /// Fixed simulation step in seconds, in `[1/1000, 1]`; default `1/60`.
    pub fixed_step: f32,
    /// Maximum fixed steps per [`PhysicsWorld::step`] call, in `[1, 120]`; default 5. Extra catch-up time is dropped.
    pub max_sub_steps: u32,
}

impl Default for PhysicsWorldOptions {
    fn default() -> Self {
        Self {
            gravity: DEFAULT_GRAVITY,
            fixed_step: DEFAULT_FIXED_STEP,
            max_sub_steps: DEFAULT_MAX_SUB_STEPS,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Owner
// ---------------------------------------------------------------------------------------------------------------------

/// Bookkeeping for one owned body.
pub(super) struct BodyRecord {
    /// The body's single collider.
    pub(super) collider: ColliderHandle,
    /// Whether the body is dynamic (mass > 0).
    pub(super) dynamic: bool,
    /// The body's pose before the most recent fixed step, for presentation between steps. Creation and teleports set
    /// it to the new pose so a body never blends across a jump.
    pub(super) previous_pose: Pose,
    /// The raycast vehicle this body carries when it was created as a vehicle chassis; it lives and dies with the body.
    pub(super) vehicle: Option<VehicleRecord>,
}

/// Bookkeeping for the raycast vehicle a chassis body carries.
pub(super) struct VehicleRecord {
    /// Solver controller bound to the chassis body; its wheel order matches `wheels`.
    pub(super) controller: DynamicRayCastVehicleController,
    /// Validated wheel configuration (friction slip resolved).
    pub(super) wheels: Vec<WheelOptions>,
    /// Drive force in newtons at full throttle.
    pub(super) engine_force: f32,
    /// Steering angle in radians at full steer.
    pub(super) max_steer: f32,
    /// Brake impulse bound at full brake.
    pub(super) brake_force: f32,
}

/// Owns a rapier solver and every body created through it, including the raycast vehicles chassis bodies carry.
///
/// Independent instances share no simulation state. Collisions are discrete: thin surfaces and very fast bodies can
/// tunnel, and raycast tires have no solid wheel collider. Dropping the world releases everything it owns.
pub struct PhysicsWorld {
    // Identity stamped into every handle this world issues.
    pub(super) world_id: u64,

    // Fixed-step policy and catch-up state (seconds; the accumulator is f64 so rounding does not shift step counts).
    fixed_step: f32,
    max_sub_steps: u32,
    accumulator: f64,

    // Solver state. Bodies (and the vehicles some of them carry) are kept in insertion order so iteration is
    // deterministic.
    pub(super) solver: rapier3d::pipeline::PhysicsWorld,
    pub(super) bodies: IndexMap<RigidBodyHandle, BodyRecord>,

    // Bodies with user forces pending; forces apply for exactly one fixed step, like cannon-es.
    pub(super) forced_bodies: Vec<RigidBodyHandle>,

    // Static boxes made resident through the `PhysicsBackend` contract, keyed by shape id, and the newest applied
    // mutation batch sequence.
    pub(super) resident_shapes: IndexMap<String, BodyHandle>,
    pub(super) last_mutation_sequence: Option<u64>,

    // SDF contact-search diagnostics for the latest fixed step, shared with the solver's narrow-phase dispatcher.
    sdf_counters: Arc<SdfCollisionCounters>,
}

impl fmt::Debug for PhysicsWorld {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PhysicsWorld")
            .field("world_id", &self.world_id)
            .field("fixed_step", &self.fixed_step)
            .field("max_sub_steps", &self.max_sub_steps)
            .field("accumulator", &self.accumulator)
            .field("bodies", &self.bodies.len())
            .field("vehicles", &self.vehicle_count())
            .field("resident_shapes", &self.resident_shapes.len())
            .finish()
    }
}

impl PhysicsWorld {
    /// Create an independent Y-up solver with bounded fixed-step work.
    ///
    /// Fails if gravity is non-finite, the fixed step is outside `[1/1000, 1]` seconds, or `max_sub_steps` is outside
    /// `[1, 120]`.
    pub fn new(options: PhysicsWorldOptions) -> Result<Self, PhysicsError> {
        // Fixed work limits prevent hidden-tab catch-up from stalling the frame.
        let fixed_step = finite_number(options.fixed_step, "fixedStep", MIN_FIXED_STEP, MAX_FIXED_STEP)?;
        if options.max_sub_steps < 1 || options.max_sub_steps > MAX_SUB_STEPS_LIMIT {
            return Err(PhysicsError::OutOfRange {
                name: "maxSubSteps",
                minimum: 1.0,
                maximum: f64::from(MAX_SUB_STEPS_LIMIT),
            });
        }
        let gravity = finite_vector(options.gravity, "gravity")?;

        let mut solver = rapier3d::pipeline::PhysicsWorld::new();
        solver.gravity = to_solver_vector(gravity);
        solver.integration_parameters.dt = fixed_step;

        // SDF pairs are handled first; every other pair falls through to the solver's default dispatcher unchanged.
        let sdf_counters = Arc::new(SdfCollisionCounters::default());
        let dispatcher = SdfQueryDispatcher::new(Arc::clone(&sdf_counters)).chain(DefaultQueryDispatcher);
        solver.narrow_phase = NarrowPhase::with_query_dispatcher(dispatcher);

        Ok(Self {
            world_id: NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed),
            fixed_step,
            max_sub_steps: options.max_sub_steps,
            accumulator: 0.0,
            solver,
            bodies: IndexMap::new(),
            forced_bodies: Vec::new(),
            resident_shapes: IndexMap::new(),
            last_mutation_sequence: None,
            sdf_counters,
        })
    }

    /// Advance by real elapsed seconds and return the number of fixed steps executed.
    ///
    /// Elapsed time accumulates and is clamped to `fixed_step * max_sub_steps`, so excess catch-up time is discarded.
    /// Each fixed step updates every vehicle (in creation order), advances the solver, and then clears user forces.
    /// `elapsed_seconds` must be finite and nonnegative; on failure no state changes.
    pub fn step(&mut self, elapsed_seconds: f32) -> Result<u32, PhysicsError> {
        finite_number(elapsed_seconds, "elapsedSeconds", 0.0, f64::INFINITY)?;
        let fixed_step = f64::from(self.fixed_step);
        let budget = fixed_step * f64::from(self.max_sub_steps);
        self.accumulator = (self.accumulator + f64::from(elapsed_seconds)).min(budget);

        let mut steps = 0;
        while self.accumulator + ACCUMULATOR_EPSILON >= fixed_step && steps < self.max_sub_steps {
            self.advance_fixed_step();
            self.accumulator = (self.accumulator - fixed_step).max(0.0);
            steps += 1;
        }
        Ok(steps)
    }

    /// Closest hit along the world-space segment from `from` to `to`, or `None`.
    ///
    /// A body whose shape contains the segment start is ignored (cannon-es `skipBackfaces` semantics). Non-finite or
    /// zero-length segments return `None`. The result is a copy; the body handle is borrowed. Queries read current
    /// body poses, so bodies created or teleported since the last step are found immediately.
    pub fn raycast(&self, from: Vec3, to: Vec3) -> Option<RaycastHit> {
        self.raycast_bodies(from, to, self.bodies.keys().copied())
    }

    /// Number of live bodies owned by this world.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Number of live bodies that carry a raycast vehicle.
    pub fn vehicle_count(&self) -> usize {
        self.bodies.values().filter(|record| record.vehicle.is_some()).count()
    }

    /// Fixed simulation step in seconds.
    pub fn fixed_step(&self) -> f32 {
        self.fixed_step
    }

    /// How far presentation is between the previous fixed step and the latest one, in `[0, 1)`: the accumulated time
    /// that has not yet made a whole step, as a fraction of one.
    ///
    /// Presentation that blends each body from its pose before the latest step toward its current pose by this
    /// fraction ([`interpolated_body_transform`](Self::interpolated_body_transform)) moves smoothly on displays whose
    /// refresh rate is not a multiple of the simulation rate, at the cost of trailing the simulation by up to one step.
    pub fn interpolation(&self) -> f32 {
        let fixed_step = f64::from(self.fixed_step);
        (self.accumulator / fixed_step).clamp(0.0, 1.0) as f32
    }

    /// Maximum fixed steps executed per [`PhysicsWorld::step`] call.
    pub fn max_sub_steps(&self) -> u32 {
        self.max_sub_steps
    }

    /// World gravity in meters per second squared.
    pub fn gravity(&self) -> Vec3 {
        from_solver_vector(self.solver.gravity)
    }

    /// Copied SDF contact-search counts from the most recent fixed step; all zero before the first step or without
    /// SDF bodies.
    pub fn sdf_collision_statistics(&self) -> SdfCollisionStatistics {
        self.sdf_counters.snapshot()
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Internal helpers shared by the body and vehicle modules
    // -----------------------------------------------------------------------------------------------------------------

    /// Resolve a public handle to a live solver handle owned by this world.
    pub(super) fn resolve_body(&self, handle: BodyHandle) -> Option<RigidBodyHandle> {
        if handle.world != self.world_id || !self.bodies.contains_key(&handle.raw) {
            return None;
        }
        Some(handle.raw)
    }

    /// Closest hit among `candidates` along the segment from `from` to `to`, with [`PhysicsWorld::raycast`] rules.
    ///
    /// Ties keep the earliest candidate, so callers control precedence through iteration order.
    pub(super) fn raycast_bodies(
        &self,
        from: Vec3,
        to: Vec3,
        candidates: impl Iterator<Item = RigidBodyHandle>,
    ) -> Option<RaycastHit> {
        if !from.is_finite() || !to.is_finite() {
            return None;
        }
        let delta = to - from;
        let length = delta.length();
        if length <= 0.0 || !length.is_finite() {
            return None;
        }
        let ray = Ray::new(to_solver_vector(from), to_solver_vector(delta / length));

        // Every candidate is tested against its current pose. Levels hold few enough bodies that this linear scan is
        // cheap, and it avoids stale broad-phase data between steps.
        let mut closest: Option<RaycastHit> = None;
        for handle in candidates {
            let Some(record) = self.bodies.get(&handle) else {
                continue;
            };
            let (Some(body), Some(collider)) =
                (self.solver.bodies.get(handle), self.solver.colliders.get(record.collider))
            else {
                continue;
            };
            let pose = match collider.position_wrt_parent() {
                Some(local) => *body.position() * *local,
                None => *collider.position(),
            };
            let Some(hit) = collider.shape().cast_ray_and_get_normal(&pose, &ray, length, true) else {
                continue;
            };

            // A zero time of impact means the segment starts inside the shape; skip it like a backface.
            if hit.time_of_impact <= 0.0 {
                continue;
            }
            if closest.is_some_and(|best| best.distance <= hit.time_of_impact) {
                continue;
            }
            closest = Some(RaycastHit {
                body: BodyHandle {
                    world: self.world_id,
                    raw: handle,
                },
                point: from + ((delta / length) * hit.time_of_impact),
                normal: from_solver_vector(hit.normal),
                distance: hit.time_of_impact,
            });
        }
        closest
    }

    /// Execute one fixed step: vehicles first (cannon's pre-step), then the solver, then clear one-step forces.
    fn advance_fixed_step(&mut self) {
        let dt = self.fixed_step;

        // Vehicles update in their chassis bodies' creation order.
        for (chassis, record) in self.bodies.iter_mut() {
            let Some(vehicle) = record.vehicle.as_mut() else {
                continue;
            };

            // Wheel rays ignore their own chassis but may land on any other body, static or dynamic.
            let filter = QueryFilter::default().exclude_rigid_body(*chassis);
            let queries = self.solver.broad_phase.as_query_pipeline_mut(
                self.solver.narrow_phase.query_dispatcher(),
                &mut self.solver.bodies,
                &mut self.solver.colliders,
                filter,
            );
            vehicle.controller.update_vehicle(dt, queries);
        }

        // Remember where every body starts this step; presentation blends from here toward the solved pose.
        for (handle, record) in self.bodies.iter_mut() {
            if let Some(body) = self.solver.bodies.get(*handle) {
                record.previous_pose = *body.position();
            }
        }

        self.sdf_counters.reset();
        self.solver.step();

        for handle in self.forced_bodies.drain(..) {
            if let Some(body) = self.solver.bodies.get_mut(handle) {
                body.reset_forces(false);
                body.reset_torques(false);
            }
        }
    }
}
