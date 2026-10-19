// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The entity owner: spawning with rollback, runtime components, removal, deterministic insertion-order updates,
//! entity systems, and exclusive controller possession.
//!
//! Lifecycle and control contracts:
//! - Spawning registers the entity, attaches runtime components for authored types the [`ComponentRegistry`] knows (in
//!   authored order), and then runs the behavior's `on_spawn`. Any failure removes the entity again, running the same
//!   teardown as [`EntityWorld::remove`], before the error is returned.
//! - Entities update in insertion order. Entities spawned during an update wait for the next update; entities removed
//!   during an update are skipped for the rest of it. Systems run first, in the order they were added.
//! - Possession is exclusive. Possessing an entity displaces its previous controller; both the previously and the newly
//!   possessed entities have their control intent cleared and their behavior's `reset_control` called. Removed entities
//!   cannot be possessed, and removing an entity releases its controller.
//!
//! Every entity can be possessed. The world owns possession bookkeeping.

use std::collections::HashSet;
use std::fmt;

use indexmap::IndexMap;

use super::character::ControlIntent;
use super::component::EntityComponent;
use super::component_registry::ComponentRegistry;
use super::entity::{
    ControllerId,
    Entity,
    EntityBehavior,
    EntityCommand,
    EntityId,
    EntityState,
    SimulationContext,
};
use super::entity_definition::EntityDefinition;
use super::entity_registry::EntityRegistry;
use super::entity_system::{
    EntitySystem,
    SystemId,
};
use super::error::GameplayError;

/// Caller name used in delta-time errors.
const WORLD_DELTA: &str = "World delta";

/// Owns entities, controllers, and systems; independent of rendering and input.
pub struct EntityWorld {
    // Factories for spawned entity types and authored component types.
    registry: EntityRegistry,
    component_registry: ComponentRegistry,

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
    /// An empty world that spawns through `registry` and attaches no runtime components.
    pub fn new(registry: EntityRegistry) -> Self {
        Self::with_component_registry(registry, ComponentRegistry::new())
    }

    /// An empty world that spawns through `registry` and attaches runtime components from `component_registry`.
    pub fn with_component_registry(registry: EntityRegistry, component_registry: ComponentRegistry) -> Self {
        Self {
            registry,
            component_registry,
            entities: IndexMap::new(),
            next_serial: 1,
            controllers: IndexMap::new(),
            next_controller: 1,
            systems: Vec::new(),
            next_system: 1,
            pending: Vec::new(),
        }
    }

    /// The entity factory registry.
    pub fn registry(&self) -> &EntityRegistry {
        &self.registry
    }

    /// The entity factory registry, for registering types after construction.
    pub fn registry_mut(&mut self) -> &mut EntityRegistry {
        &mut self.registry
    }

    /// The runtime component factory registry.
    pub fn component_registry(&self) -> &ComponentRegistry {
        &self.component_registry
    }

    /// The runtime component factory registry, for registering types after construction. Components attach only to
    /// entities spawned after registration.
    pub fn component_registry_mut(&mut self) -> &mut ComponentRegistry {
        &mut self.component_registry
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Lifetime
    // -----------------------------------------------------------------------------------------------------------------

    /// Create and own an entity from a copied definition, returning its id.
    ///
    /// The definition is validated, the registered factory creates the behavior, the entity is registered, runtime
    /// components attach in authored order, and then `on_spawn` runs. If a component's `on_attach` or the behavior's
    /// `on_spawn` fails, the entity is removed (releasing possession, running `on_destroy`, and detaching attached
    /// components) before the error is returned. Fails without side effects for invalid definitions, duplicate ids,
    /// and unknown types.
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
        self.entities.insert(id.clone(), Entity { state, behavior, components: IndexMap::new(), serial });

        if let Err(error) = self.attach_registered_components(&id, context) {
            self.remove(&id, context);
            return Err(error);
        }
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
    /// `on_destroy` runs, then runtime components detach in reverse attachment order (each `on_detach`, then drop), and
    /// finally the body the context lists for the entity (with any vehicle it carries) is removed from physics.
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
        while let Some((_, mut component)) = entity.components.pop() {
            component.on_detach(&mut entity.state, context);
        }

        // The body listed for the entity (and the vehicle a chassis carries) is entity-owned.
        if let Some(body) = context.body(id) {
            context.physics.remove_body(body);
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

        // Systems are taken out so they can borrow entities mutably; they cannot add or remove systems meanwhile.
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
    // Runtime components
    // -----------------------------------------------------------------------------------------------------------------

    /// Attach a runtime component to a live entity under `kind`, then run its `on_attach`.
    ///
    /// Fails for an unknown entity, a blank type, or a type the entity already carries. If `on_attach` fails, the
    /// component is detached again (running `on_detach`) and dropped before the error is returned; the entity stays.
    pub fn add_component(
        &mut self,
        id: &EntityId,
        kind: &str,
        component: Box<dyn EntityComponent>,
        context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        if !self.entities.contains_key(id) {
            return Err(GameplayError::UnknownEntity(id.clone()));
        }
        if kind.trim().is_empty() {
            return Err(GameplayError::EmptyComponentType);
        }
        self.attach_component(id, kind, component, context)?;
        self.pending.extend(context.take_commands());
        Ok(())
    }

    /// Detach and drop a runtime component, running its `on_detach`. Returns whether it was attached.
    ///
    /// The entity's authored record for the type is kept.
    pub fn remove_component(&mut self, id: &EntityId, kind: &str, context: &mut SimulationContext<'_>) -> bool {
        let Some(entity) = self.entities.get_mut(id) else {
            return false;
        };
        let Some(mut component) = entity.components.shift_remove(kind) else {
            return false;
        };
        component.on_detach(&mut entity.state, context);
        self.pending.extend(context.take_commands());
        true
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

    /// Live entities carrying every listed runtime component type, in insertion order (a stable snapshot).
    ///
    /// Authored records without a runtime component do not match.
    pub fn query(&self, component_types: &[&str]) -> Vec<&Entity> {
        self.entities.values().filter(|entity| has_components(entity, component_types)).collect()
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Systems
    // -----------------------------------------------------------------------------------------------------------------

    /// Add a system at the end of the update order and run its `on_add`.
    ///
    /// Fails for a blank required component type, or when `on_add` fails; then the system is removed again (running
    /// `on_remove`) and dropped.
    pub fn add_system(&mut self, mut system: Box<dyn EntitySystem>) -> Result<SystemId, GameplayError> {
        if system.required_components().iter().any(|kind| kind.trim().is_empty()) {
            return Err(GameplayError::InvalidSystemRequirement);
        }
        if let Err(error) = system.on_add() {
            system.on_remove();
            return Err(error);
        }
        let id = SystemId(self.next_system);
        self.next_system += 1;
        self.systems.push((id, system));
        Ok(id)
    }

    /// Remove a system, running its `on_remove`. Returns whether it was present.
    pub fn remove_system(&mut self, id: SystemId) -> bool {
        let Some(index) = self.systems.iter().position(|(candidate, _)| *candidate == id) else {
            return false;
        };
        let (_, mut system) = self.systems.remove(index);
        system.on_remove();
        true
    }

    /// Remove every system in reverse order, running each `on_remove`; the world does this before releasing entities.
    pub fn clear_systems(&mut self) {
        while let Some((_, mut system)) = self.systems.pop() {
            system.on_remove();
        }
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

    /// Replace an entity's control intent.
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

    /// Attach runtime components for every authored type the component registry knows, in authored order.
    ///
    /// A record removed by an earlier component's `on_attach` is skipped.
    fn attach_registered_components(
        &mut self,
        id: &EntityId,
        context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        let kinds: Vec<String> = match self.entities.get(id) {
            None => return Ok(()),
            Some(entity) => {
                entity.state.components.keys().filter(|kind| self.component_registry.contains(kind)).cloned().collect()
            }
        };
        for kind in kinds {
            let Some(entity) = self.entities.get(id) else {
                return Ok(());
            };
            let Some(record) = entity.state.components.get(&kind) else {
                continue;
            };
            let component = self.component_registry.create(&kind, record, &entity.state)?;
            self.attach_component(id, &kind, component, context)?;
        }
        Ok(())
    }

    /// Insert a component and run `on_attach`; a failure detaches it again (running `on_detach`).
    fn attach_component(
        &mut self,
        id: &EntityId,
        kind: &str,
        component: Box<dyn EntityComponent>,
        context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        let entity = self.entities.get_mut(id).ok_or_else(|| GameplayError::UnknownEntity(id.clone()))?;
        if entity.components.contains_key(kind) {
            return Err(GameplayError::DuplicateComponent { entity: id.clone(), kind: kind.to_owned() });
        }
        entity.components.insert(kind.to_owned(), component);

        let Entity { state, components, .. } = entity;
        let attached = components.get_mut(kind).expect("the component was inserted above");
        if let Err(error) = attached.on_attach(state, context) {
            if let Some(mut failed) = components.shift_remove(kind) {
                failed.on_detach(state, context);
            }
            return Err(error);
        }
        Ok(())
    }

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
            let mut matches: Vec<&mut Entity> = self
                .entities
                .values_mut()
                .filter(|entity| frame_serials.contains(&entity.serial) && has_components(entity, required))
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

impl Drop for EntityWorld {
    fn drop(&mut self) {
        // Systems need no simulation context to release; entities are released by the composition root (`World`),
        // which owns the physics their teardown needs.
        self.clear_systems();
    }
}

impl fmt::Debug for EntityWorld {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntityWorld")
            .field("entities", &self.entities.keys().collect::<Vec<_>>())
            .field("controllers", &self.controllers)
            .field("systems", &self.systems.len())
            .field("component_registry", &self.component_registry)
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

/// Whether an entity carries every listed runtime component.
fn has_components(entity: &Entity, component_types: &[&str]) -> bool {
    component_types.iter().all(|kind| entity.has_component(kind))
}
