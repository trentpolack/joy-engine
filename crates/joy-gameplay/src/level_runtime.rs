// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Instantiates an authored level: every entity, with a rigid body for each `physics` record (a raycast vehicle chassis
//! when the entity also has a `vehicle` record), owned by a [`World`] that advances them in its fixed order.
//!
//! - Bodies come from the engine's built-in `physics` and `vehicle` runtime components (see the `physics_component`
//!   contract in `docs/rust-simulation.md`), so entities spawned later by the game or requested by behaviors through
//!   `SimulationContext::request_spawn` receive the same physics setup as authored ones.
//! - Every failure rolls back: a failed spawn removes the entity (physics validates a body and its vehicle together, so
//!   no partial body is left behind), and a failed level setup removes every entity created so far.

use super::entity::EntityId;
use super::entity_definition::EntityDefinition;
use super::entity_registry::EntityRegistry;
use super::entity_world::EntityWorld;
use super::error::GameplayError;
use super::level_document::LevelData;
use super::world::World;
use joy_physics::{
    BodyHandle,
    PhysicsSimulation,
    PhysicsSimulationOptions,
};

/// Caller name used in delta-time errors.
const RUNTIME_DELTA: &str = "Runtime delta";

/// Owns a copied level and the world its entities and bodies live in.
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
        Self::with_physics_options(level, registry, PhysicsSimulationOptions::default())
    }

    /// Validate and copy a level, then spawn every entity in authored order with its physics.
    ///
    /// Fails if the level is invalid, still links object templates (resolve them with `resolve_level_objects` first),
    /// or any entity fails to spawn; on failure every entity created so far is removed (running `on_destroy`) and the
    /// physics simulation is released.
    pub fn with_physics_options(
        level: &LevelData,
        registry: EntityRegistry,
        physics: PhysicsSimulationOptions,
    ) -> Result<Self, GameplayError> {
        level.validate()?;
        if let Some(linked) = level.entities.iter().find(|entity| entity.template.is_some()) {
            return Err(GameplayError::UnresolvedTemplate(EntityId::new(linked.id.clone())));
        }

        let mut runtime = Self {
            level: level.clone(),
            world: World::new(registry, physics)?,
        };
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
        self.world.spawn(definition)
    }

    /// Remove an entity and its body. Returns whether it was live.
    pub fn remove(&mut self, id: &EntityId) -> bool {
        self.world.remove(id)
    }

    /// Advance one simulation boundary of `seconds` (finite, nonnegative) through [`World::update`]: entities update,
    /// physics steps (bounded by its own catch-up limit), every live body's position and rotation are copied into its
    /// entity's transform, and contacts become events. Update
    /// controllers before calling this. Authored level data is never modified.
    pub fn step(&mut self, seconds: f32) -> Result<(), GameplayError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(GameplayError::InvalidDelta(RUNTIME_DELTA));
        }
        self.world.update(seconds)?;
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

    /// The physics simulation.
    pub fn physics(&self) -> &PhysicsSimulation {
        &self.world.physics
    }

    /// The physics simulation, mutably (for vehicle input, impulses, or raycasts).
    pub fn physics_mut(&mut self) -> &mut PhysicsSimulation {
        &mut self.world.physics
    }
}
