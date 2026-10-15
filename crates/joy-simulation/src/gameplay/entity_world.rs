// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The entity owner: spawning with rollback, removal, deterministic insertion-order updates, entity systems, and
//! exclusive controller possession.
//!
//! Semantics follow the TypeScript `EntityWorld`, `Character`, and `Controller`:
//! - Entities update in insertion order. Entities spawned during an update wait for the next update; entities removed
//!   during an update are skipped for the rest of it.
//! - Possession is exclusive. Possessing an entity displaces its previous controller; both the previously and the newly
//!   possessed entities have their control intent cleared and their behavior's `reset_control` called. Removed entities
//!   cannot be possessed, and removing an entity releases its controller.
//!
//! In Rust every entity can be possessed (the TypeScript engine restricts possession to `Character` subclasses), and
//! the world, not the entity, owns possession bookkeeping.

use std::collections::HashSet;
use std::fmt;

use indexmap::IndexMap;

use super::definition::EntityDefinition;
use super::entity::{
    ControlIntent, ControllerId, Entity, EntityBehavior, EntityCommand, EntityId, EntityState, SimulationContext,
};
use super::error::GameplayError;
use super::registry::EntityRegistry;

/// Caller name used in delta-time errors.
const WORLD_DELTA: &str = "World delta";

/// Identifier of a system added with [`EntityWorld::add_system`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SystemId(u64);

/// World-owned processor for entities selected by component composition.
///
/// Systems run before entity behaviors in every update, in the order they were added, and receive a stable
/// insertion-order list of matching entities. Matching uses the entity's authored component records (the TypeScript
/// engine matched runtime components created by a separate `ComponentRegistry`, which this crate does not port).
pub trait EntitySystem {
    /// Component types an entity must carry to be passed to [`EntitySystem::update`]; empty matches every entity.
    fn required_components(&self) -> &[&str] {
        &[]
    }

    /// Process matching entities by nonnegative simulation seconds. Structural changes go through `context`.
    fn update(&mut self, elapsed_seconds: f32, matches: &mut [&mut EntityState], context: &mut SimulationContext<'_>);
}

/// Owns entities, controllers, and systems; independent of rendering and input.
pub struct EntityWorld {
    // Factories for spawned types.
    registry: EntityRegistry,

    // Live entities in insertion order, plus the serial assigned to the next spawn.
    entities: IndexMap<EntityId, Entity>,
    next_serial: u64,

    // Controllers and the entity each one possesses.
    controllers: IndexMap<ControllerId, Option<EntityId>>,
    next_controller: u64,

    // Systems in update order.
    systems: Vec<(SystemId, Box<dyn EntitySystem>)>,
    next_system: u64,

    // Structural requests made from spawn and destroy hooks, applied at the start of the next update.
    pending: Vec<EntityCommand>,
}

impl EntityWorld {
    /// An empty world that spawns through `registry`.
    pub fn new(registry: EntityRegistry) -> Self {
        Self {
            registry,
            entities: IndexMap::new(),
            next_serial: 1,
            controllers: IndexMap::new(),
            next_controller: 1,
            systems: Vec::new(),
            next_system: 1,
            pending: Vec::new(),
        }
    }

    /// The factory registry.
    pub fn registry(&self) -> &EntityRegistry {
        &self.registry
    }

    /// The factory registry, for registering types after construction.
    pub fn registry_mut(&mut self) -> &mut EntityRegistry {
        &mut self.registry
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Lifetime
    // -----------------------------------------------------------------------------------------------------------------

    /// Create and own an entity from a copied definition, returning its id.
    ///
    /// The definition is validated, the registered factory creates the behavior, the entity is registered, and then
    /// `on_spawn` runs. If `on_spawn` fails, the entity is removed (releasing possession and running `on_destroy`)
    /// before the error is returned. Fails without side effects for invalid definitions, duplicate ids, and unknown
    /// types.
    pub fn spawn(
        &mut self,
        definition: &EntityDefinition,
        context: &mut SimulationContext<'_>,
    ) -> Result<EntityId, GameplayError> {
        definition.validate()?;
        let id = EntityId::new(definition.id.clone());
        if self.entities.contains_key(&id) {
            return Err(GameplayError::DuplicateEntity(id));
        }
        let behavior = self.registry.create(definition)?;

        let serial = self.next_serial;
        self.next_serial += 1;
        let state = EntityState::from_definition(definition);
        self.entities.insert(id.clone(), Entity { state, behavior, serial });

        let spawn_result = self.with_behavior(&id, |behavior, state| behavior.on_spawn(state, context));
        if let Some(Err(error)) = spawn_result {
            self.remove(&id, context);
            return Err(error);
        }
        self.pending.extend(context.take_commands());
        Ok(id)
    }

    /// Remove an entity, returning whether it was live.
    ///
    /// Ownership is detached first, then possession is released (clearing intent and calling `reset_control`), then
    /// `on_destroy` runs, and finally any body and vehicle the context lists for the entity are removed from physics.
    pub fn remove(&mut self, id: &EntityId, context: &mut SimulationContext<'_>) -> bool {
        let Some(mut entity) = self.entities.shift_remove(id) else {
            return false;
        };

        if let Some(controller) = entity.state.possessed_by.take()
            && let Some(slot) = self.controllers.get_mut(&controller)
        {
            *slot = None;
        }
        reset_entity_control(&mut entity);
        if let Some(behavior) = entity.behavior.as_mut() {
            behavior.on_destroy(&mut entity.state, context);
        }

        // Physics handles listed for the entity are entity-owned resources (TypeScript `entity.own`).
        if let Some(body) = context.body(id) {
            context.physics.remove_body(body);
        }
        if let Some(vehicle) = context.vehicle(id) {
            context.physics.remove_vehicle(vehicle);
        }
        self.pending.extend(context.take_commands());
        true
    }

    /// Remove every entity in insertion order, running each removal's hooks.
    pub fn clear(&mut self, context: &mut SimulationContext<'_>) {
        let ids: Vec<EntityId> = self.entities.keys().cloned().collect();
        for id in ids {
            self.remove(&id, context);
        }
        self.pending.clear();
    }

    /// Advance systems, then entity behaviors, by finite nonnegative simulation seconds.
    ///
    /// Requests deferred from earlier spawn or destroy hooks are applied first. The frame's entity list is captured
    /// next: systems run in order with their matches, then each surviving entity's behavior updates in insertion order.
    /// Requests made during the frame are applied right after the requesting system or behavior returns. On failure no
    /// state changes if the delta is invalid; otherwise the frame stops at the failing request.
    pub fn update(&mut self, elapsed_seconds: f32, context: &mut SimulationContext<'_>) -> Result<(), GameplayError> {
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            return Err(GameplayError::InvalidDelta(WORLD_DELTA));
        }
        self.apply_pending(context)?;

        let frame: Vec<(EntityId, u64)> =
            self.entities.iter().map(|(id, entity)| (id.clone(), entity.serial)).collect();
        let frame_serials: HashSet<u64> = frame.iter().map(|(_, serial)| *serial).collect();

        // Systems are taken out so they can borrow entity state mutably; they cannot add or remove systems meanwhile.
        let mut systems = std::mem::take(&mut self.systems);
        let system_result = self.run_systems(&mut systems, &frame_serials, elapsed_seconds, context);
        systems.append(&mut self.systems);
        self.systems = systems;
        system_result?;

        for (id, serial) in &frame {
            let live = self.entities.get(id).is_some_and(|entity| entity.serial == *serial);
            if !live {
                continue;
            }
            self.with_behavior(id, |behavior, state| behavior.update(state, context, elapsed_seconds));
            self.apply_commands(context)?;
        }
        Ok(())
    }

    /// Apply structural requests deferred from spawn and destroy hooks, plus any left in `context`.
    pub fn apply_pending(&mut self, context: &mut SimulationContext<'_>) -> Result<(), GameplayError> {
        while !self.pending.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            for command in pending {
                self.apply_command(command, context)?;
            }
        }
        self.apply_commands(context)
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------------------------------------------------

    /// Borrow a live entity.
    pub fn get(&self, id: &EntityId) -> Option<&Entity> {
        self.entities.get(id)
    }

    /// Borrow a live entity mutably.
    pub fn get_mut(&mut self, id: &EntityId) -> Option<&mut Entity> {
        self.entities.get_mut(id)
    }

    /// Whether an entity is live.
    pub fn contains(&self, id: &EntityId) -> bool {
        self.entities.contains_key(id)
    }

    /// Live entities in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    /// Live entities in insertion order, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Entity> {
        self.entities.values_mut()
    }

    /// Number of live entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether no entities are live.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Live entities carrying every listed component type, in insertion order.
    pub fn query(&self, component_types: &[&str]) -> Vec<&Entity> {
        self.entities.values().filter(|entity| has_components(&entity.state, component_types)).collect()
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Systems
    // -----------------------------------------------------------------------------------------------------------------

    /// Add a system at the end of the update order.
    pub fn add_system(&mut self, system: Box<dyn EntitySystem>) -> SystemId {
        let id = SystemId(self.next_system);
        self.next_system += 1;
        self.systems.push((id, system));
        id
    }

    /// Remove a system, returning whether it was present.
    pub fn remove_system(&mut self, id: SystemId) -> bool {
        let before = self.systems.len();
        self.systems.retain(|(candidate, _)| *candidate != id);
        self.systems.len() != before
    }

    /// Number of systems.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Control and possession
    // -----------------------------------------------------------------------------------------------------------------

    /// Create an unpossessing controller.
    pub fn create_controller(&mut self) -> ControllerId {
        let id = ControllerId(self.next_controller);
        self.next_controller += 1;
        self.controllers.insert(id, None);
        id
    }

    /// Transfer exclusive possession to `target`, or release possession with `None`.
    ///
    /// Every backreference is committed before any hook runs: the controller's previous entity and the target's
    /// previous controller are detached, and both the previous and new entities have their intent cleared and
    /// `reset_control` called. Possessing the current target is a no-op. Fails for a released controller (unless
    /// releasing) or an entity that is not live; failures change nothing.
    pub fn possess(&mut self, controller: ControllerId, target: Option<&EntityId>) -> Result<(), GameplayError> {
        let Some(current) = self.controllers.get(&controller).cloned() else {
            return match target {
                None => Ok(()),
                Some(_) => Err(GameplayError::UnknownController),
            };
        };
        if let Some(id) = target
            && !self.entities.contains_key(id)
        {
            return Err(GameplayError::InvalidPossessionTarget);
        }
        if current.as_ref() == target {
            return Ok(());
        }

        let previous = current;
        let displaced = target.and_then(|id| self.entities.get(id)).and_then(|entity| entity.state.possessed_by);

        // Commit ownership on every side before running project hooks.
        self.controllers.insert(controller, target.cloned());
        if let Some(entity) = previous.as_ref().and_then(|id| self.entities.get_mut(id)) {
            entity.state.possessed_by = None;
            entity.state.control_intent.clear();
        }
        if let Some(slot) = displaced.and_then(|other| self.controllers.get_mut(&other)) {
            *slot = None;
        }
        if let Some(entity) = target.and_then(|id| self.entities.get_mut(id)) {
            entity.state.possessed_by = Some(controller);
            entity.state.control_intent.clear();
        }

        for changed in [previous.as_ref(), target].into_iter().flatten() {
            if let Some(entity) = self.entities.get_mut(changed) {
                reset_entity_control(entity);
            }
        }
        Ok(())
    }

    /// The entity a controller possesses, if any.
    pub fn possessed(&self, controller: ControllerId) -> Option<&EntityId> {
        self.controllers.get(&controller)?.as_ref()
    }

    /// Whether a controller exists (was created here and not released).
    pub fn has_controller(&self, controller: ControllerId) -> bool {
        self.controllers.contains_key(&controller)
    }

    /// Release a controller's possession and retire it. Repeated calls are harmless.
    pub fn release_controller(&mut self, controller: ControllerId) {
        // Releasing possession cannot fail for a live controller.
        let _ = self.possess(controller, None);
        self.controllers.shift_remove(&controller);
    }

    /// Replace an entity's control intent, as the TypeScript `receiveControl` does.
    ///
    /// The previous intent is cleared and `reset_control` runs first; a non-finite numeric action then fails and leaves
    /// the intent empty.
    pub fn receive_control(&mut self, id: &EntityId, intent: ControlIntent) -> Result<(), GameplayError> {
        let entity = self.entities.get_mut(id).ok_or_else(|| GameplayError::UnknownEntity(id.clone()))?;
        reset_entity_control(entity);
        intent.validate()?;
        entity.state.control_intent = intent;
        Ok(())
    }

    /// Clear an entity's control intent and call its behavior's `reset_control`. Returns whether the entity is live.
    pub fn reset_control(&mut self, id: &EntityId) -> bool {
        let Some(entity) = self.entities.get_mut(id) else {
            return false;
        };
        reset_entity_control(entity);
        true
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------------------------------------------------

    /// Run one closure with an entity's behavior taken out, so it can borrow the entity's state mutably.
    fn with_behavior<T>(
        &mut self,
        id: &EntityId,
        run: impl FnOnce(&mut dyn EntityBehavior, &mut EntityState) -> T,
    ) -> Option<T> {
        let entity = self.entities.get_mut(id)?;
        let mut behavior = entity.behavior.take()?;
        let result = run(behavior.as_mut(), &mut entity.state);
        // The behavior had no access to the world, so its entity is still registered here.
        if let Some(entity) = self.entities.get_mut(id) {
            entity.behavior = Some(behavior);
        }
        Some(result)
    }

    /// Run every system over its matches from the frame snapshot, applying requests after each.
    fn run_systems(
        &mut self,
        systems: &mut [(SystemId, Box<dyn EntitySystem>)],
        frame_serials: &HashSet<u64>,
        elapsed_seconds: f32,
        context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        for (_, system) in systems.iter_mut() {
            let required = system.required_components();
            let mut matches: Vec<&mut EntityState> = self
                .entities
                .values_mut()
                .filter(|entity| frame_serials.contains(&entity.serial) && has_components(&entity.state, required))
                .map(|entity| &mut entity.state)
                .collect();
            system.update(elapsed_seconds, &mut matches, context);
            self.apply_commands(context)?;
        }
        Ok(())
    }

    /// Apply the requests currently queued in `context`, in order.
    fn apply_commands(&mut self, context: &mut SimulationContext<'_>) -> Result<(), GameplayError> {
        for command in context.take_commands() {
            self.apply_command(command, context)?;
        }
        Ok(())
    }

    /// Apply one structural request.
    fn apply_command(
        &mut self,
        command: EntityCommand,
        context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        match command {
            EntityCommand::Spawn(definition) => self.spawn(&definition, context).map(|_| ()),
            EntityCommand::Remove(id) => {
                self.remove(&id, context);
                Ok(())
            }
        }
    }
}

impl Default for EntityWorld {
    fn default() -> Self {
        Self::new(EntityRegistry::new())
    }
}

impl fmt::Debug for EntityWorld {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntityWorld")
            .field("entities", &self.entities.keys().collect::<Vec<_>>())
            .field("controllers", &self.controllers)
            .field("systems", &self.systems.len())
            .finish()
    }
}

/// Clear intent and call the behavior's reset hook.
fn reset_entity_control(entity: &mut Entity) {
    entity.state.control_intent.clear();
    if let Some(behavior) = entity.behavior.as_mut() {
        behavior.reset_control(&mut entity.state);
    }
}

/// Whether an entity carries every listed authored component.
fn has_components(state: &EntityState, component_types: &[&str]) -> bool {
    component_types.iter().all(|kind| state.components.contains_key(*kind))
}
