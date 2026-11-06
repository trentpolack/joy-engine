// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Headless simulation root: entities, physics, events, and timers, advanced in one fixed order.
//!
//! One [`World::update`] is one simulation boundary:
//! 1. The previous update's events become the delivered set behaviors read, and due timers fire.
//! 2. Systems, then entity behaviors, run with the elapsed time.
//! 3. Physics steps with the full elapsed time (bounded by its own catch-up limit), applying forces behaviors set.
//! 4. Every owned body's pose is written back to its entity's transform, so the next update's behaviors read the
//!    solved state.
//! 5. Contacts the physics step reported become [`ContactEvent`]s naming the entities involved.
//!
//! Drive it from a fixed tick (`App::fixed_update` with the tick's seconds) so every update simulates the same amount of
//! time; physics then runs exactly one fixed step per update when their rates match. Changing this order changes
//! simulation results.

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
use super::world_events::{
    ContactEvent,
    EventQueue,
    WorldEvents,
};
use super::world_timers::{
    TimerId,
    TimerRepeat,
    WorldTimers,
};
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
/// `on_destroy` hooks and detaching components), and then releases physics.
#[derive(Debug)]
pub struct World {
    /// Entity owner.
    pub entities: EntityWorld,
    /// Physics owner.
    pub physics: PhysicsWorld,
    /// Body owned by each entity, including vehicle chassis bodies.
    pub bodies: IndexMap<EntityId, BodyHandle>,
    // Events produced and delivered at update boundaries, and running timers.
    events: WorldEvents,
    timers: WorldTimers,
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
        Self {
            entities,
            physics,
            bodies: IndexMap::new(),
            events: WorldEvents::default(),
            timers: WorldTimers::default(),
        }
    }

    /// Borrow the entity world and a simulation context built from the other fields.
    pub fn split(&mut self) -> (&mut EntityWorld, SimulationContext<'_>) {
        let Self {
            entities,
            physics,
            bodies,
            events,
            timers,
        } = self;
        (entities, SimulationContext::new(physics, bodies, events, timers))
    }

    /// Advance one simulation boundary by finite, nonnegative seconds, in the order the module documentation lists.
    ///
    /// Fails without changing state for an invalid delta. If a structural request fails during the entity update, the
    /// update stops there and physics does not step.
    pub fn update(&mut self, elapsed_seconds: f32) -> Result<WorldStep, GameplayError> {
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            return Err(GameplayError::InvalidDelta(WORLD_DELTA));
        }

        // Deliver the previous update's events and fire due timers into this update's output.
        self.events.advance();
        self.timers.advance(f64::from(elapsed_seconds), &mut self.events);

        // Gameplay acts on the state the last physics step solved. Frame spikes are already bounded by the shell's clock
        // (and fixed ticks never see them), so entities receive exactly the time physics does.
        let (entities, mut context) = self.split();
        let result = entities.update(elapsed_seconds, &mut context);
        self.prune_handles();
        result?;

        // Physics consumes the forces and velocities behaviors set, then hands the solved poses and contacts back.
        let physics_steps = self.physics.step(elapsed_seconds)?;
        self.write_back_poses();
        self.report_contacts();
        Ok(WorldStep {
            elapsed_seconds,
            physics_steps,
        })
    }

    /// Events the latest update produced (plus any the game sent since), readable by type.
    pub fn events(&self) -> &EventQueue {
        self.events.produced()
    }

    /// Send an event from outside an update; behaviors read it during the next update.
    pub fn send_event<T: 'static>(&mut self, event: T) {
        self.events.send(event);
    }

    /// Start a timer that fires a [`TimerFired`](super::TimerFired) event after `seconds` of simulation time, counted
    /// from the next update. An owned timer is cancelled when its entity is removed.
    pub fn start_timer(
        &mut self,
        owner: Option<EntityId>,
        key: impl Into<std::borrow::Cow<'static, str>>,
        seconds: f64,
        repeat: TimerRepeat,
    ) -> TimerId {
        self.timers.start(owner, key, seconds, repeat)
    }

    /// Stop a running timer. Returns whether it was running.
    pub fn cancel_timer(&mut self, timer: TimerId) -> bool {
        self.timers.cancel(timer)
    }

    /// The running timers.
    pub fn timers(&self) -> &WorldTimers {
        &self.timers
    }

    /// The entity that owns a body, if any.
    pub fn entity_for_body(&self, body: BodyHandle) -> Option<&EntityId> {
        self.bodies.iter().find(|(_, owned)| **owned == body).map(|(id, _)| id)
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
        self.entities.iter().map(|entity| PresentedEntity {
            state: &entity.state,
            transform: self.blended_transform(&entity.state),
        })
    }

    /// Spawn a copied definition; `physics` and `vehicle` records create the entity's body.
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
        let Self {
            entities,
            physics,
            bodies,
            ..
        } = self;
        bodies.retain(|id, body| entities.contains(id) && physics.contains_body(*body));
    }

    /// Remove every entity in insertion order, running their hooks, and forget their handles, timers, and events.
    pub fn clear(&mut self) {
        let (entities, mut context) = self.split();
        entities.clear(&mut context);
        self.bodies.clear();
        self.timers.clear();
        self.events.clear();
    }
}

impl World {
    /// Copy every owned body's solved position and rotation into its entity, forgetting handles of removed entities or
    /// bodies.
    fn write_back_poses(&mut self) {
        let Self {
            entities,
            physics,
            bodies,
            ..
        } = self;
        bodies.retain(|id, body| {
            let (Some(entity), Some(pose)) = (entities.get_mut(id), physics.body_transform(*body)) else {
                return false;
            };
            entity.state.transform.position = pose.position;
            entity.state.transform.rotation = pose.rotation;
            true
        });
    }

    /// Translate the physics step's contact events into entity contact events.
    fn report_contacts(&mut self) {
        if self.physics.events().is_empty() {
            return;
        }
        let owners: std::collections::HashMap<BodyHandle, &EntityId> =
            self.bodies.iter().map(|(id, body)| (*body, id)).collect();
        let contacts: Vec<ContactEvent> = self
            .physics
            .events()
            .iter()
            .map(|event| ContactEvent {
                phase: event.phase,
                first: owners.get(&event.first).map(|id| (*id).clone()),
                second: owners.get(&event.second).map(|id| (*id).clone()),
                first_body: event.first,
                second_body: event.second,
                sensor: event.sensor,
            })
            .collect();
        for contact in contacts {
            self.events.send(contact);
        }
    }

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
        // Systems release first, and entities release before the physics they own.
        self.entities.clear_systems();
        self.clear();
    }
}
