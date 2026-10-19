// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Instantiates an authored level: entities, and a rigid body for each `physics` component, which is a raycast vehicle
//! chassis when the entity also has a `vehicle` component.
//!
//! - `physics` components use local collider dimensions; the entity scale is applied once at spawn. Box half extents
//!   scale per axis; spheres require uniform scale and scale their radius. The body starts at the entity's position and
//!   rotation.
//! - `vehicle` components follow the [`VehicleOptions`] contract in chassis-local meters; they require a `physics`
//!   component and uniform scale, which scales wheel position, radius, suspension rest length, and maximum suspension
//!   travel. Force tuning stays in authored units.
//! - Every failure rolls back: a failed spawn removes the entity (physics validates a body and its vehicle together, so
//!   no partial body is left behind), and a failed level setup removes every entity created so far.
//! - Spawn dynamic entities through [`LevelRuntime::spawn`] so they receive the same physics setup as authored ones.
//!   Entities spawned by behaviors through `SimulationContext::request_spawn` bypass physics setup, as do entities
//!   spawned directly on `EntityWorld`.

use joy_core::math::Vec3;
use joy_core::{
    JsonRecord,
    PrimitiveShape,
};
use serde_json::Value;

use super::component_vocabulary::component_type;
use super::entity::EntityId;
use super::entity_definition::{
    EntityDefinition,
    read_vector,
};
use super::entity_registry::EntityRegistry;
use super::entity_world::EntityWorld;
use super::error::{
    GameplayError,
    PhysicsComponentError,
};
use super::level_document::LevelData;
use super::world::World;
use joy_physics::{
    BODY_DEFAULTS,
    BodyHandle,
    BodyOptions,
    PhysicsWorld,
    PhysicsWorldOptions,
    VEHICLE_DEFAULTS,
    VehicleOptions,
    WHEEL_DEFAULTS,
    WheelOptions,
};

/// Longest entity update per step in seconds; physics still receives the full elapsed time and bounds its own catch-up.
const MAX_ENTITY_STEP_SECONDS: f32 = 0.1;

/// Caller name used in delta-time errors.
const RUNTIME_DELTA: &str = "Runtime delta";

/// Owns a copied level, its entity world, its physics world, and every entity's body handle.
#[derive(Debug)]
pub struct LevelRuntime {
    // The validated, copied level this runtime was built from; never mutated.
    level: LevelData,

    // Simulation root. Dropping it removes entities before physics.
    world: World,
}

impl LevelRuntime {
    /// Instantiate a level with default physics options. See [`LevelRuntime::with_physics_options`].
    pub fn new(level: &LevelData, registry: EntityRegistry) -> Result<Self, GameplayError> {
        Self::with_physics_options(level, registry, PhysicsWorldOptions::default())
    }

    /// Validate and copy a level, then spawn every entity in authored order with its physics.
    ///
    /// Fails if the level is invalid, still links object templates (resolve them with `resolve_level_objects` first),
    /// or any entity fails to spawn; on failure every entity created so far is removed (running `on_destroy`) and the
    /// physics world is released.
    pub fn with_physics_options(
        level: &LevelData,
        registry: EntityRegistry,
        physics: PhysicsWorldOptions,
    ) -> Result<Self, GameplayError> {
        level.validate()?;
        if let Some(linked) = level.entities.iter().find(|entity| entity.template.is_some()) {
            return Err(GameplayError::UnresolvedTemplate(EntityId::new(linked.id.clone())));
        }

        let mut runtime = Self { level: level.clone(), world: World::new(registry, physics)? };
        let definitions = runtime.level.entities.clone();
        for definition in &definitions {
            if let Err(error) = runtime.spawn(definition) {
                runtime.world.clear();
                return Err(error);
            }
        }
        Ok(runtime)
    }

    /// Spawn an entity and create the body its components describe.
    ///
    /// If physics setup fails, the entity is removed before the error is returned, so the world is unchanged.
    pub fn spawn(&mut self, definition: &EntityDefinition) -> Result<EntityId, GameplayError> {
        let id = self.world.spawn(definition)?;
        if let Err(source) = self.attach_physics(&id) {
            self.world.remove(&id);
            return Err(GameplayError::PhysicsComponent { entity: id, source });
        }
        Ok(id)
    }

    /// Remove an entity and its body. Returns whether it was live.
    pub fn remove(&mut self, id: &EntityId) -> bool {
        self.world.remove(id)
    }

    /// Advance one frame of `seconds` (finite, nonnegative).
    ///
    /// Exactly like the TypeScript runtime: entities update with `min(seconds, 0.1)`, physics steps with the full
    /// `seconds` (bounded by its own catch-up limit), and then every live body's position and rotation are copied into
    /// its entity's transform. Handles of removed entities or bodies are dropped. Update controllers before calling
    /// this. Authored level data is never modified.
    pub fn step(&mut self, seconds: f32) -> Result<(), GameplayError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(GameplayError::InvalidDelta(RUNTIME_DELTA));
        }
        {
            let (entities, mut context) = self.world.split();
            entities.update(seconds.min(MAX_ENTITY_STEP_SECONDS), &mut context)?;
        }
        self.world.physics.step(seconds)?;

        let world = &mut self.world;
        let mut stale = Vec::new();
        for (id, body) in &world.bodies {
            let transform = world.physics.body_transform(*body);
            match (world.entities.get_mut(id), transform) {
                (Some(entity), Some(transform)) => {
                    entity.state.transform.position = transform.position;
                    entity.state.transform.rotation = transform.rotation;
                }
                _ => stale.push(id.clone()),
            }
        }
        for id in stale {
            world.bodies.shift_remove(&id);
        }
        Ok(())
    }

    /// The live body owned by an entity, if both are live.
    ///
    /// For a vehicle entity this is the chassis: vehicle operations (`physics().wheel_transforms(body)` for rendering,
    /// `vehicle_speed`, `set_vehicle_input`) take it directly.
    pub fn body(&self, id: &EntityId) -> Option<BodyHandle> {
        if !self.world.entities.contains(id) {
            return None;
        }
        self.world.bodies.get(id).copied().filter(|body| self.world.physics.contains_body(*body))
    }

    /// The copied level this runtime was built from.
    pub fn level(&self) -> &LevelData {
        &self.level
    }

    /// The simulation root.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The simulation root, mutably (for `split`, custom bodies, or systems).
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The entity world.
    pub fn entities(&self) -> &EntityWorld {
        &self.world.entities
    }

    /// The entity world, mutably (for controllers and possession).
    pub fn entities_mut(&mut self) -> &mut EntityWorld {
        &mut self.world.entities
    }

    /// The physics world.
    pub fn physics(&self) -> &PhysicsWorld {
        &self.world.physics
    }

    /// The physics world, mutably (for vehicle input, impulses, or raycasts).
    pub fn physics_mut(&mut self) -> &mut PhysicsWorld {
        &mut self.world.physics
    }

    /// Create the body an entity's components describe (a vehicle chassis when it has a `vehicle` component), recording
    /// its handle.
    fn attach_physics(&mut self, id: &EntityId) -> Result<(), PhysicsComponentError> {
        let Some(entity) = self.world.entities.get(id) else {
            return Ok(());
        };
        let collider = entity.state.components.get(component_type::PHYSICS).cloned();
        let vehicle_config = entity.state.components.get(component_type::VEHICLE).cloned();
        let transform = entity.state.transform;
        if vehicle_config.is_some() && collider.is_none() {
            return Err(PhysicsComponentError::VehicleWithoutPhysics);
        }
        let Some(collider) = collider else {
            return Ok(());
        };

        let options = body_options(&collider, transform.position, transform.rotation, transform.scale)?;
        let body = match vehicle_config {
            None => self.world.physics.create_body(options)?,
            Some(config) => {
                let scale = transform.scale;
                if scale.x != scale.y || scale.y != scale.z {
                    return Err(PhysicsComponentError::NonUniformVehicleScale);
                }
                let vehicle = vehicle_options(&config, scale.x)?;
                self.world.physics.create_vehicle(options, vehicle)?
            }
        };
        self.world.bodies.insert(id.clone(), body);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Component interpretation
// ---------------------------------------------------------------------------------------------------------------------

/// Read a `physics` component into body options, applying entity scale once.
fn body_options(
    config: &JsonRecord,
    position: Vec3,
    rotation: Vec3,
    scale: Vec3,
) -> Result<BodyOptions, PhysicsComponentError> {
    if scale.min_element() <= 0.0 {
        return Err(PhysicsComponentError::NonPositiveScale);
    }
    let shape = config
        .get("shape")
        .and_then(Value::as_str)
        .and_then(PrimitiveShape::parse)
        .ok_or(PhysicsComponentError::InvalidShape)?;
    let mut options = BodyOptions { shape, position, rotation, ..BODY_DEFAULTS };

    // Shape dimensions are checked before material values, in the TypeScript order.
    match shape {
        PrimitiveShape::Box => {
            let extents = match present(config, "halfExtents") {
                None => BODY_DEFAULTS.half_extents,
                Some(value) => read_vector(value)
                    .filter(|extents| extents.min_element() > 0.0)
                    .ok_or(PhysicsComponentError::InvalidHalfExtents)?,
            };
            options.half_extents = extents * scale;
        }
        PrimitiveShape::Sphere => {
            if scale.x != scale.y || scale.y != scale.z {
                return Err(PhysicsComponentError::NonUniformSphereScale);
            }
            let radius = match present(config, "radius") {
                None => BODY_DEFAULTS.radius,
                Some(value) => value
                    .as_f64()
                    .map(|radius| radius as f32)
                    .filter(|radius| radius.is_finite() && *radius > 0.0)
                    .ok_or(PhysicsComponentError::InvalidRadius)?,
            };
            options.radius = radius * scale.x;
        }
        PrimitiveShape::Plane => {}
    }

    options.mass = number_field(config, "mass", BODY_DEFAULTS.mass)?;
    options.friction = number_field(config, "friction", BODY_DEFAULTS.friction)?;
    options.restitution = number_field(config, "restitution", BODY_DEFAULTS.restitution)?;
    options.linear_damping = number_field(config, "linearDamping", BODY_DEFAULTS.linear_damping)?;
    options.angular_damping = number_field(config, "angularDamping", BODY_DEFAULTS.angular_damping)?;
    Ok(options)
}

/// Read a `vehicle` component into vehicle options, scaling tire geometry with a uniform chassis scale.
fn vehicle_options(config: &JsonRecord, scale: f32) -> Result<VehicleOptions, PhysicsComponentError> {
    let Some(Value::Array(authored_wheels)) = config.get("wheels") else {
        return Err(PhysicsComponentError::InvalidWheels);
    };

    // Geometry first (the TypeScript `scaleVehicle` pass), then tuning and flags.
    let mut wheels = Vec::with_capacity(authored_wheels.len());
    for value in authored_wheels {
        let Value::Object(wheel) = value else {
            return Err(PhysicsComponentError::InvalidWheel);
        };
        let position =
            wheel.get("position").and_then(read_vector).ok_or(PhysicsComponentError::InvalidWheelPosition)?;
        let radius = wheel_dimension(wheel, "radius", WHEEL_DEFAULTS.radius)?;
        let suspension_rest_length =
            wheel_dimension(wheel, "suspensionRestLength", WHEEL_DEFAULTS.suspension_rest_length)?;
        let max_suspension_travel =
            wheel_dimension(wheel, "maxSuspensionTravel", WHEEL_DEFAULTS.max_suspension_travel)?;
        wheels.push((
            wheel,
            WheelOptions {
                position: position * scale,
                radius: radius * scale,
                suspension_rest_length: suspension_rest_length * scale,
                max_suspension_travel: max_suspension_travel * scale,
                ..WHEEL_DEFAULTS
            },
        ));
    }

    let mut options = VehicleOptions {
        wheels: Vec::with_capacity(wheels.len()),
        engine_force: number_field(config, "engineForce", VEHICLE_DEFAULTS.engine_force)?,
        max_steer: number_field(config, "maxSteer", VEHICLE_DEFAULTS.max_steer)?,
        brake_force: number_field(config, "brakeForce", VEHICLE_DEFAULTS.brake_force)?,
        friction_slip: number_field(config, "frictionSlip", VEHICLE_DEFAULTS.friction_slip)?,
    };
    for (wheel, mut parsed) in wheels {
        parsed.steering = wheel_flag(wheel, "steering", WHEEL_DEFAULTS.steering)?;
        parsed.driven = wheel_flag(wheel, "driven", WHEEL_DEFAULTS.driven)?;
        parsed.brake = wheel_flag(wheel, "brake", WHEEL_DEFAULTS.brake)?;
        parsed.suspension_stiffness = number_field(wheel, "suspensionStiffness", WHEEL_DEFAULTS.suspension_stiffness)?;
        parsed.damping_compression = number_field(wheel, "dampingCompression", WHEEL_DEFAULTS.damping_compression)?;
        parsed.damping_relaxation = number_field(wheel, "dampingRelaxation", WHEEL_DEFAULTS.damping_relaxation)?;
        parsed.max_suspension_force = number_field(wheel, "maxSuspensionForce", WHEEL_DEFAULTS.max_suspension_force)?;
        parsed.roll_influence = number_field(wheel, "rollInfluence", WHEEL_DEFAULTS.roll_influence)?;
        parsed.friction_slip = match present(wheel, "frictionSlip") {
            None => None,
            Some(value) => Some(as_number(value, "frictionSlip")?),
        };
        options.wheels.push(parsed);
    }
    Ok(options)
}

/// A field that is present and not `null` (JavaScript `??` treats both as omitted).
fn present<'a>(record: &'a JsonRecord, key: &str) -> Option<&'a Value> {
    record.get(key).filter(|value| !value.is_null())
}

/// Read an optional numeric field; range checks happen in the physics world.
fn number_field(record: &JsonRecord, key: &'static str, fallback: f32) -> Result<f32, PhysicsComponentError> {
    match present(record, key) {
        None => Ok(fallback),
        Some(value) => as_number(value, key),
    }
}

/// Require a JSON number (no string or boolean coercion).
fn as_number(value: &Value, key: &'static str) -> Result<f32, PhysicsComponentError> {
    value.as_f64().map(|number| number as f32).ok_or(PhysicsComponentError::NotANumber(key))
}

/// Read a wheel length that must be finite and nonnegative before scaling.
fn wheel_dimension(wheel: &JsonRecord, key: &str, fallback: f32) -> Result<f32, PhysicsComponentError> {
    match present(wheel, key) {
        None => Ok(fallback),
        Some(value) => value
            .as_f64()
            .map(|number| number as f32)
            .filter(|number| number.is_finite() && *number >= 0.0)
            .ok_or(PhysicsComponentError::InvalidWheelDimensions),
    }
}

/// Read an optional wheel flag; a present non-boolean (including `null`) is invalid, as in the TypeScript engine.
fn wheel_flag(wheel: &JsonRecord, key: &'static str, fallback: bool) -> Result<bool, PhysicsComponentError> {
    match wheel.get(key) {
        None => Ok(fallback),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(PhysicsComponentError::InvalidWheelFlag(key)),
    }
}
