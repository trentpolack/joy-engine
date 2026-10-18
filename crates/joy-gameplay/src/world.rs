// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Headless simulation root: entities plus physics, with the entity-to-handle maps behaviors read.

use indexmap::IndexMap;

use super::entity::{
    EntityId,
    SimulationContext,
};
use super::entity_definition::EntityDefinition;
use super::entity_registry::EntityRegistry;
use super::entity_world::EntityWorld;
use super::error::GameplayError;
use joy_physics::{
    BodyHandle,
    PhysicsWorld,
    PhysicsWorldOptions,
    VehicleHandle,
};

/// Caller name used in delta-time errors.
const WORLD_DELTA: &str = "World delta";

/// Summary of one [`World::update`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldStep {
    /// The simulation seconds that were requested.
    pub elapsed_seconds: f32,
    /// Fixed physics steps that executed.
    pub physics_steps: u32,
}

/// Owns entity simulation and physics without browser, input, or rendering facilities.
///
/// `bodies` and `vehicles` map entities to the solver objects they own; behaviors read them through
/// [`SimulationContext`], and removing an entity removes its listed body and vehicle. Fields are public so composition
/// roots can borrow them independently; use [`World::split`] to obtain an entity world and context together.
///
/// Dropping the world removes every system (running `on_remove` in reverse order), then every entity (running
/// `on_destroy` hooks and detaching components), and then releases physics, like the TypeScript `World.destroy`.
#[derive(Debug)]
pub struct World {
    /// Entity owner.
    pub entities: EntityWorld,
    /// Physics owner.
    pub physics: PhysicsWorld,
    /// Body owned by each entity.
    pub bodies: IndexMap<EntityId, BodyHandle>,
    /// Vehicle owned by each entity.
    pub vehicles: IndexMap<EntityId, VehicleHandle>,
}

impl World {
    /// Create independent entity and physics owners.
    pub fn new(registry: EntityRegistry, physics: PhysicsWorldOptions) -> Result<Self, GameplayError> {
        Ok(Self::with_physics(registry, PhysicsWorld::new(physics)?))
    }

    /// Create a world around an existing physics world, which it takes ownership of.
    pub fn with_physics(registry: EntityRegistry, physics: PhysicsWorld) -> Self {
        Self::from_parts(EntityWorld::new(registry), physics)
    }

    /// Create a world from an existing entity world (for example one built with
    /// [`EntityWorld::with_component_registry`]) and physics world, taking ownership of both.
    pub fn from_parts(entities: EntityWorld, physics: PhysicsWorld) -> Self {
        Self { entities, physics, bodies: IndexMap::new(), vehicles: IndexMap::new() }
    }

    /// Borrow the entity world and a simulation context built from the other fields.
    pub fn split(&mut self) -> (&mut EntityWorld, SimulationContext<'_>) {
        (&mut self.entities, SimulationContext::new(&mut self.physics, &self.bodies, &self.vehicles))
    }

    /// Advance physics, then entities, by finite nonnegative simulation seconds.
    ///
    /// Physics runs first so systems and behaviors observe the completed solver state, matching the TypeScript
    /// `World.update`. Fails without changing state for an invalid delta.
    pub fn update(&mut self, elapsed_seconds: f32) -> Result<WorldStep, GameplayError> {
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            return Err(GameplayError::InvalidDelta(WORLD_DELTA));
        }
        let physics_steps = self.physics.step(elapsed_seconds)?;
        let (entities, mut context) = self.split();
        let result = entities.update(elapsed_seconds, &mut context);
        self.prune_handles();
        result?;
        Ok(WorldStep { elapsed_seconds, physics_steps })
    }

    /// Spawn an entity (no physics setup; see `LevelRuntime` for component-driven bodies).
    pub fn spawn(&mut self, definition: &EntityDefinition) -> Result<EntityId, GameplayError> {
        let (entities, mut context) = self.split();
        entities.spawn(definition, &mut context)
    }

    /// Remove an entity and the body and vehicle it owns. Returns whether it was live.
    pub fn remove(&mut self, id: &EntityId) -> bool {
        let (entities, mut context) = self.split();
        let removed = entities.remove(id, &mut context);
        if removed {
            self.bodies.shift_remove(id);
            self.vehicles.shift_remove(id);
        }
        removed
    }

    /// Forget handles whose entity was removed or whose solver object no longer exists.
    pub fn prune_handles(&mut self) {
        let Self { entities, physics, bodies, vehicles } = self;
        bodies.retain(|id, body| entities.contains(id) && physics.contains_body(*body));
        vehicles.retain(|id, vehicle| entities.contains(id) && physics.contains_vehicle(*vehicle));
    }

    /// Remove every entity in insertion order, running their hooks, and forget their handles.
    pub fn clear(&mut self) {
        let (entities, mut context) = self.split();
        entities.clear(&mut context);
        self.bodies.clear();
        self.vehicles.clear();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // Systems release first and entities before the physics they own, like the TypeScript `World.destroy`.
        self.entities.clear_systems();
        self.clear();
    }
}
