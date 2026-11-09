// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The engine's runtime component for authored `physics` records: every entity that carries a `physics` record gets its
//! rigid body however it was spawned.
//!
//! - `physics` records use local collider dimensions; the entity scale is applied once at spawn. Box half extents
//!   scale per axis; spheres require uniform scale and scale their radius. The body starts at the entity's position and
//!   rotation.
//! - Optional `sensor`, `reportContacts`, `canSleep`, `collisionMembership`, and `collisionFilter` fields map to the
//!   matching `BodyOptions`; contacts of reporting bodies reach gameplay as [`ContactEvent`](super::ContactEvent)s.
//! - Attachment validates the body before creating it, so a failed spawn leaves no partial body behind. Detachment
//!   removes the body.
//!
//! A project that registers its own factory for `physics` replaces this built-in.

use joy_core::math::Vec3;
use joy_core::{
    JsonRecord,
    PrimitiveShape,
};
use joy_physics::{
    BODY_DEFAULTS,
    BodyHandle,
    BodyOptions,
    CollisionGroups,
    PhysicsSimulation,
};
use serde_json::Value;

use super::component::EntityComponent;
use super::component_vocabulary::component_type;
use super::entity::{
    EntityState,
    SimulationContext,
};
use super::entity_definition::read_vector;
use super::error::{
    GameplayError,
    PhysicsComponentError,
};

/// Authored field names of the `physics` component, shared by the reader and the editor schema.
pub mod physics_field {
    /// Collider shape: `box`, `sphere`, or `plane`.
    pub const SHAPE: &str = "shape";
    /// Mass in kilograms; zero is static.
    pub const MASS: &str = "mass";
    /// Box half extents in meters.
    pub const HALF_EXTENTS: &str = "halfExtents";
    /// Sphere radius in meters.
    pub const RADIUS: &str = "radius";
    /// Friction coefficient.
    pub const FRICTION: &str = "friction";
    /// Restitution in `[0, 1]`.
    pub const RESTITUTION: &str = "restitution";
    /// Per-second linear velocity loss fraction.
    pub const LINEAR_DAMPING: &str = "linearDamping";
    /// Per-second angular velocity loss fraction.
    pub const ANGULAR_DAMPING: &str = "angularDamping";
    /// Whether the body only reports overlaps.
    pub const SENSOR: &str = "sensor";
    /// Whether the body's contacts are reported as events.
    pub const REPORT_CONTACTS: &str = "reportContacts";
    /// Whether the body may sleep at rest.
    pub const CAN_SLEEP: &str = "canSleep";
    /// Bit mask of the collision layers the body belongs to.
    pub const COLLISION_MEMBERSHIP: &str = "collisionMembership";
    /// Bit mask of the collision layers the body interacts with.
    pub const COLLISION_FILTER: &str = "collisionFilter";
}

// ---------------------------------------------------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------------------------------------------------

/// Owns the rigid body an entity's `physics` record describes.
#[derive(Debug, Default)]
pub struct PhysicsBodyComponent {
    body: Option<BodyHandle>,
}

impl PhysicsBodyComponent {
    /// The body this component created, while attached.
    pub fn body(&self) -> Option<BodyHandle> {
        self.body
    }
}

impl EntityComponent for PhysicsBodyComponent {
    fn on_attach(
        &mut self,
        entity: &mut EntityState,
        context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        let body = create_entity_body(entity, context.physics).map_err(|source| GameplayError::PhysicsComponent {
            entity: entity.id().clone(),
            source,
        })?;
        if let Some(body) = body {
            context.attach_body(entity.id(), body);
            self.body = Some(body);
        }
        Ok(())
    }

    fn on_detach(&mut self, entity: &mut EntityState, context: &mut SimulationContext<'_>) {
        if let Some(body) = self.body.take() {
            context.detach_body(entity.id(), body);
        }
    }
}

/// Whether the engine provides a runtime component for an authored type: `physics` here, and `script` when the
/// `game-script` feature is enabled.
pub(crate) fn is_built_in_component(kind: &str) -> bool {
    kind == component_type::PHYSICS || (cfg!(feature = "game-script") && kind == component_type::SCRIPT)
}

/// The engine's runtime component for an authored type, when it provides one.
pub(crate) fn built_in_component(kind: &str) -> Option<Box<dyn EntityComponent>> {
    match kind {
        component_type::PHYSICS => Some(Box::new(PhysicsBodyComponent::default())),
        #[cfg(feature = "game-script")]
        component_type::SCRIPT => Some(Box::new(super::script_system::ScriptComponent::default())),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Body Creation
// ---------------------------------------------------------------------------------------------------------------------

/// Create the body an entity's `physics` record describes, or `None` without one.
fn create_entity_body(
    entity: &EntityState,
    physics: &mut PhysicsSimulation,
) -> Result<Option<BodyHandle>, PhysicsComponentError> {
    let Some(collider) = entity.components.get(component_type::PHYSICS) else {
        return Ok(None);
    };
    let transform = entity.transform;
    let options = body_options(collider, transform.position, transform.rotation, transform.scale)?;
    Ok(Some(physics.create_body(options)?))
}

/// Read a `physics` record into body options, applying entity scale once.
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
        .get(physics_field::SHAPE)
        .and_then(Value::as_str)
        .and_then(PrimitiveShape::parse)
        .ok_or(PhysicsComponentError::InvalidShape)?;
    let mut options = BodyOptions {
        shape,
        position,
        rotation,
        ..BODY_DEFAULTS
    };

    // Shape dimensions are checked before material values, so the first failure reported is stable.
    match shape {
        PrimitiveShape::Box => {
            let extents = match present(config, physics_field::HALF_EXTENTS) {
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
            let radius = match present(config, physics_field::RADIUS) {
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
    options.mass = number_field(config, physics_field::MASS, BODY_DEFAULTS.mass)?;
    options.friction = number_field(config, physics_field::FRICTION, BODY_DEFAULTS.friction)?;
    options.restitution = number_field(config, physics_field::RESTITUTION, BODY_DEFAULTS.restitution)?;
    options.linear_damping = number_field(config, physics_field::LINEAR_DAMPING, BODY_DEFAULTS.linear_damping)?;
    options.angular_damping = number_field(config, physics_field::ANGULAR_DAMPING, BODY_DEFAULTS.angular_damping)?;

    // Event, sleep, and layer options.
    options.sensor = flag(config, physics_field::SENSOR, BODY_DEFAULTS.sensor)?;
    options.report_contacts = flag(config, physics_field::REPORT_CONTACTS, BODY_DEFAULTS.report_contacts)?;
    options.can_sleep = flag(config, physics_field::CAN_SLEEP, BODY_DEFAULTS.can_sleep)?;
    options.groups = CollisionGroups::new(
        mask(config, physics_field::COLLISION_MEMBERSHIP, BODY_DEFAULTS.groups.membership)?,
        mask(config, physics_field::COLLISION_FILTER, BODY_DEFAULTS.groups.filter)?,
    );
    Ok(options)
}

fn present<'a>(record: &'a JsonRecord, key: &str) -> Option<&'a Value> {
    record.get(key).filter(|value| !value.is_null())
}

/// Read an optional numeric field; range checks happen in the physics simulation.
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

/// Read an optional boolean field; a present non-boolean is invalid.
fn flag(record: &JsonRecord, key: &'static str, fallback: bool) -> Result<bool, PhysicsComponentError> {
    match present(record, key) {
        None => Ok(fallback),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(PhysicsComponentError::NotABoolean(key)),
    }
}

/// Read an optional collision mask: a whole number in `[0, 2^32)`.
fn mask(record: &JsonRecord, key: &'static str, fallback: u32) -> Result<u32, PhysicsComponentError> {
    match present(record, key) {
        None => Ok(fallback),
        Some(value) => value
            .as_u64()
            .and_then(|mask| u32::try_from(mask).ok())
            .ok_or(PhysicsComponentError::InvalidCollisionMask(key)),
    }
}
