// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime entities: identity, mutable state, control intent, project behavior, and the context behaviors run in.
//!
//! An [`Entity`] is split into its [`EntityState`] (plain data) and an optional boxed [`EntityBehavior`] (project
//! logic). The world takes the behavior out while it runs, so behaviors receive `&mut EntityState` and a
//! [`SimulationContext`] without aliasing the world that owns them. Behaviors refer to other entities, bodies, and
//! vehicles by id or handle, never by reference.

use std::borrow::Borrow;
use std::fmt;

use indexmap::IndexMap;
use joy_foundation::JsonRecord;

use super::definition::{EntityDefinition, EntityTransform};
use super::error::GameplayError;
use crate::physics::{BodyHandle, PhysicsWorld, VehicleHandle};

// ---------------------------------------------------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------------------------------------------------

/// Stable entity identifier, unique within one [`EntityWorld`](super::EntityWorld).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(String);

impl EntityId {
    /// Wrap an identifier string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<&str> for EntityId {
    fn from(id: &str) -> Self {
        Self(id.to_owned())
    }
}

impl From<String> for EntityId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl Borrow<str> for EntityId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Identifier of a controller created by [`EntityWorld::create_controller`](super::EntityWorld::create_controller).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ControllerId(pub(crate) u64);

// ---------------------------------------------------------------------------------------------------------------------
// Control intent
// ---------------------------------------------------------------------------------------------------------------------

/// One named action value: an axis or a button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlValue {
    /// Analog value, for example throttle in `[-1, 1]`. Must be finite when delivered.
    Number(f64),
    /// Digital value, for example a boost button.
    Bool(bool),
}

impl ControlValue {
    /// Numeric reading: numbers as-is, `true` as 1, and `false` as 0.
    pub fn as_number(self) -> f64 {
        match self {
            Self::Number(value) => value,
            Self::Bool(flag) => f64::from(u8::from(flag)),
        }
    }

    /// Boolean reading: booleans as-is and numbers as "nonzero".
    pub fn as_bool(self) -> bool {
        match self {
            Self::Number(value) => value != 0.0,
            Self::Bool(flag) => flag,
        }
    }
}

impl From<f64> for ControlValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<f32> for ControlValue {
    fn from(value: f32) -> Self {
        Self::Number(f64::from(value))
    }
}

impl From<bool> for ControlValue {
    fn from(flag: bool) -> Self {
        Self::Bool(flag)
    }
}

/// Project-defined actions sampled by a controller, in insertion order. Each delivery replaces the whole intent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlIntent {
    values: IndexMap<String, ControlValue>,
}

impl ControlIntent {
    /// An empty intent.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form of [`ControlIntent::insert`].
    pub fn with(mut self, name: impl Into<String>, value: impl Into<ControlValue>) -> Self {
        self.insert(name, value);
        self
    }

    /// Set one action, replacing any previous value for the name.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<ControlValue>) {
        self.values.insert(name.into(), value.into());
    }

    /// The raw value of an action, if present.
    pub fn get(&self, name: &str) -> Option<ControlValue> {
        self.values.get(name).copied()
    }

    /// Numeric reading of an action: missing actions read 0, booleans read 0 or 1.
    pub fn number(&self, name: &str) -> f64 {
        self.get(name).map_or(0.0, ControlValue::as_number)
    }

    /// Boolean reading of an action: missing actions read false, numbers read "nonzero".
    pub fn flag(&self, name: &str) -> bool {
        self.get(name).is_some_and(ControlValue::as_bool)
    }

    /// Whether no actions are set.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Number of actions set.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Remove every action.
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Actions in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, ControlValue)> {
        self.values.iter().map(|(name, value)| (name.as_str(), *value))
    }

    /// Fail if any numeric action is not finite, matching the TypeScript `receiveControl` check.
    pub fn validate(&self) -> Result<(), GameplayError> {
        let finite = self.values.values().all(|value| match value {
            ControlValue::Number(number) => number.is_finite(),
            ControlValue::Bool(_) => true,
        });
        if !finite {
            return Err(GameplayError::NonFiniteIntent);
        }
        Ok(())
    }
}

impl<K: Into<String>, V: Into<ControlValue>> FromIterator<(K, V)> for ControlIntent {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut intent = Self::new();
        for (name, value) in iter {
            intent.insert(name, value);
        }
        intent
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------------------------------------------------

/// Everything about a live entity except its behavior.
///
/// The id and possession are owned by the world and read-only here; everything else is freely mutable gameplay data.
/// `components` holds the entity's own copy of its authored records; editing it never changes the source level.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityState {
    // Identity and possession, maintained by the world.
    id: EntityId,
    pub(crate) possessed_by: Option<ControllerId>,

    /// Display name.
    pub name: String,
    /// Entity type the behavior was created for.
    pub kind: String,
    /// Free-form tags.
    pub tags: Vec<String>,
    /// Current placement. `LevelRuntime` overwrites position and rotation from physics after each step.
    pub transform: EntityTransform,
    /// Entity-owned copy of the authored component records.
    pub components: IndexMap<String, JsonRecord>,
    /// Actions delivered by the possessing controller; cleared whenever possession changes.
    pub control_intent: ControlIntent,
}

impl EntityState {
    /// Build runtime state from a validated definition (copied).
    pub(crate) fn from_definition(definition: &EntityDefinition) -> Self {
        Self {
            id: EntityId::new(definition.id.clone()),
            possessed_by: None,
            name: definition.name.clone(),
            kind: definition.kind.clone(),
            tags: definition.tags.clone(),
            transform: definition.transform,
            components: definition.components.clone(),
            control_intent: ControlIntent::new(),
        }
    }

    /// Stable identifier.
    pub fn id(&self) -> &EntityId {
        &self.id
    }

    /// The controller currently possessing this entity, if any.
    pub fn possessed_by(&self) -> Option<ControllerId> {
        self.possessed_by
    }

    /// An authored component record by type.
    pub fn component(&self, kind: &str) -> Option<&JsonRecord> {
        self.components.get(kind)
    }

    /// Whether the entity carries a tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|candidate| candidate == tag)
    }

    /// Snapshot the current state back into an authored definition (no template link).
    pub fn to_definition(&self) -> EntityDefinition {
        EntityDefinition {
            id: self.id.as_str().to_owned(),
            name: self.name.clone(),
            kind: self.kind.clone(),
            tags: self.tags.clone(),
            transform: self.transform,
            components: self.components.clone(),
            template: None,
            overrides: None,
        }
    }
}

/// A live entity owned by an [`EntityWorld`](super::EntityWorld): plain state plus optional project behavior.
pub struct Entity {
    /// Gameplay state.
    pub state: EntityState,

    // Project behavior; taken out by the world while it runs so it can borrow `state` mutably.
    pub(crate) behavior: Option<Box<dyn EntityBehavior>>,

    // Spawn serial, unique for the world's lifetime; distinguishes a respawned id from the original.
    pub(crate) serial: u64,
}

impl Entity {
    /// Stable identifier.
    pub fn id(&self) -> &EntityId {
        self.state.id()
    }

    /// Whether a project behavior is attached.
    pub fn has_behavior(&self) -> bool {
        self.behavior.is_some()
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Entity")
            .field("state", &self.state)
            .field("has_behavior", &self.behavior.is_some())
            .finish()
    }
}

/// Project logic attached to an entity by an [`EntityRegistry`](super::EntityRegistry) factory.
///
/// Every hook has a default no-op implementation. Behaviors own their Rust resources (released by `Drop`) and refer to
/// solver objects through the handles in [`SimulationContext`].
pub trait EntityBehavior {
    /// Acquire resources after the entity is registered. Returning an error rolls the spawn back: possession is
    /// released, [`EntityBehavior::on_destroy`] runs, and the entity is removed before the error is returned.
    fn on_spawn(&mut self, entity: &mut EntityState, context: &mut SimulationContext<'_>) -> Result<(), GameplayError> {
        let _ = (entity, context);
        Ok(())
    }

    /// Advance by nonnegative simulation seconds. Called once per world update, in entity insertion order.
    fn update(&mut self, entity: &mut EntityState, context: &mut SimulationContext<'_>, elapsed_seconds: f32) {
        let _ = (entity, context, elapsed_seconds);
    }

    /// Release project resources; called exactly once when the entity is removed, including after a failed
    /// [`EntityBehavior::on_spawn`].
    fn on_destroy(&mut self, entity: &mut EntityState, context: &mut SimulationContext<'_>) {
        let _ = (entity, context);
    }

    /// Clear transient control state when possession changes, input fails, or the entity is removed. The world has
    /// already cleared `entity.control_intent` before calling this.
    fn reset_control(&mut self, entity: &mut EntityState) {
        let _ = entity;
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------------------------------------------------

/// Deferred structural change requested by a behavior or system.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EntityCommand {
    /// Spawn a copied definition (boxed to keep queued commands small).
    Spawn(Box<EntityDefinition>),
    /// Remove a live entity.
    Remove(EntityId),
}

/// Borrowed simulation services available to behaviors and systems.
///
/// Gives mutable access to the physics world and read access to the entity-to-body and entity-to-vehicle maps owned by
/// the composition root (`World` or `LevelRuntime`). Bodies and vehicles listed for an entity are owned by that entity:
/// removing the entity removes them from physics. Structural changes (spawning and removing entities) are requested
/// here and applied by the world: immediately after the requesting behavior or system returns during an update, or at
/// the start of the next update when requested from a spawn or destroy hook.
pub struct SimulationContext<'a> {
    /// The physics world.
    pub physics: &'a mut PhysicsWorld,
    bodies: &'a IndexMap<EntityId, BodyHandle>,
    vehicles: &'a IndexMap<EntityId, VehicleHandle>,
    commands: Vec<EntityCommand>,
}

impl<'a> SimulationContext<'a> {
    /// Borrow simulation services for one call.
    pub fn new(
        physics: &'a mut PhysicsWorld,
        bodies: &'a IndexMap<EntityId, BodyHandle>,
        vehicles: &'a IndexMap<EntityId, VehicleHandle>,
    ) -> Self {
        Self { physics, bodies, vehicles, commands: Vec::new() }
    }

    /// The live body owned by an entity, if any.
    pub fn body(&self, id: &EntityId) -> Option<BodyHandle> {
        self.bodies.get(id).copied().filter(|handle| self.physics.contains_body(*handle))
    }

    /// The live vehicle owned by an entity, if any.
    pub fn vehicle(&self, id: &EntityId) -> Option<VehicleHandle> {
        self.vehicles.get(id).copied().filter(|handle| self.physics.contains_vehicle(*handle))
    }

    /// Request that a copy of `definition` be spawned. It is not updated in the frame that requested it.
    pub fn request_spawn(&mut self, definition: EntityDefinition) {
        self.commands.push(EntityCommand::Spawn(Box::new(definition)));
    }

    /// Request that an entity be removed. Later entities in the same update no longer see it.
    pub fn request_remove(&mut self, id: EntityId) {
        self.commands.push(EntityCommand::Remove(id));
    }

    /// Take every pending request, in request order.
    pub(crate) fn take_commands(&mut self) -> Vec<EntityCommand> {
        std::mem::take(&mut self.commands)
    }
}

impl fmt::Debug for SimulationContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SimulationContext")
            .field("bodies", &self.bodies.len())
            .field("vehicles", &self.vehicles.len())
            .field("pending_commands", &self.commands.len())
            .finish()
    }
}
