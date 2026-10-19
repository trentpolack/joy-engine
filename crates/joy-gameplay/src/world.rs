// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Headless simulation root: entities plus physics, with the entity-to-handle maps behaviors read.

use indexmap::IndexMap;

use super::entity::{
    EntityId,
    EntityState,
    SimulationContext,
};
use super::entity_definition::{
    EntityDefinition,
    EntityTransform,
};
use super::entity_registry::EntityRegistry;
use super::entity_world::EntityWorld;
use super::error::GameplayError;
use joy_physics::{
    BodyHandle,
    PhysicsWorld,
    PhysicsWorldOptions,
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

/// An entity's state paired with the placement to draw it at this frame (see [`World::presented_entities`]).
#[derive(Clone, Copy, Debug)]
pub struct PresentedEntity<'a> {
    /// The entity's simulation state.
    pub state: &'a EntityState,
    /// Where to draw it. Entities with a body blend between their last two fixed physics steps; others use their
    /// current transform. Scale always comes from the entity.
    pub transform: EntityTransform,
}

/// Owns entity simulation and physics without browser, input, or rendering facilities.
///
/// `bodies` maps entities to the physics bodies they own (a vehicle is a kind of body, so a kart's chassis is listed
/// here too); behaviors read it through [`SimulationContext`], and removing an entity removes its listed body. Fields
/// are public so composition roots can borrow them independently; use [`World::split`] to obtain an entity world and
/// context together.
///
/// Dropping the world removes every system (running `on_remove` in reverse order), then every entity (running
/// `on_destroy` hooks and detaching components), and then releases physics, like the TypeScript `World.destroy`.
#[derive(Debug)]
pub struct World {
    /// Entity owner.
    pub entities: EntityWorld,
    /// Physics owner.
    pub physics: PhysicsWorld,
    /// Body owned by each entity, including vehicle chassis bodies.
    pub bodies: IndexMap<EntityId, BodyHandle>,
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
        Self { entities, physics, bodies: IndexMap::new() }
    }

    /// Borrow the entity world and a simulation context built from the other fields.
    pub fn split(&mut self) -> (&mut EntityWorld, SimulationContext<'_>) {
        (&mut self.entities, SimulationContext::new(&mut self.physics, &self.bodies))
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

    /// The placement to draw an entity at this frame, or `None` if it is not live.
    ///
    /// An entity with a body takes the body's [interpolated pose](PhysicsWorld::interpolated_body_transform), so
    /// motion stays smooth when the display refreshes faster than (or out of step with) the fixed physics rate. Gameplay
    /// keeps reading `EntityState::transform`, the simulation's state.
    pub fn presentation_transform(&self, id: &EntityId) -> Option<EntityTransform> {
        let entity = self.entities.get(id)?;
        Some(self.blended_transform(&entity.state))
    }

    /// Every live entity with its presentation placement, in insertion order, for scene synchronization.
    pub fn presented_entities(&self) -> impl Iterator<Item = PresentedEntity<'_>> {
        self.entities
            .iter()
            .map(|entity| PresentedEntity { state: &entity.state, transform: self.blended_transform(&entity.state) })
    }

    /// Spawn an entity (no physics setup; see `LevelRuntime` for component-driven bodies).
    pub fn spawn(&mut self, definition: &EntityDefinition) -> Result<EntityId, GameplayError> {
        let (entities, mut context) = self.split();
        entities.spawn(definition, &mut context)
    }

    /// Remove an entity and the body it owns. Returns whether it was live.
    pub fn remove(&mut self, id: &EntityId) -> bool {
        let (entities, mut context) = self.split();
        let removed = entities.remove(id, &mut context);
        if removed {
            self.bodies.shift_remove(id);
        }
        removed
    }

    /// Forget handles whose entity was removed or whose body no longer exists.
    pub fn prune_handles(&mut self) {
        let Self { entities, physics, bodies } = self;
        bodies.retain(|id, body| entities.contains(id) && physics.contains_body(*body));
    }

    /// Remove every entity in insertion order, running their hooks, and forget their handles.
    pub fn clear(&mut self) {
        let (entities, mut context) = self.split();
        entities.clear(&mut context);
        self.bodies.clear();
    }
}

impl World {
    /// An entity's transform with its body's interpolated pose, when it owns a live body.
    fn blended_transform(&self, state: &EntityState) -> EntityTransform {
        let mut transform = state.transform;
        let pose = self.bodies.get(state.id()).and_then(|body| self.physics.interpolated_body_transform(*body));
        if let Some(pose) = pose {
            transform.position = pose.position;
            transform.rotation = pose.rotation;
        }
        transform
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // Systems release first and entities before the physics they own, like the TypeScript `World.destroy`.
        self.entities.clear_systems();
        self.clear();
    }
}
