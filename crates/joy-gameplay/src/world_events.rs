// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Typed events a world update produces, and the engine's own event records.
//!
//! An event lives for exactly one update boundary. Everything produced during an update (events behaviors and systems
//! send, contacts from the physics step, timers that came due, and entities spawned or removed) is readable by the
//! game through [`World::events`](super::World::events) after that update returns, and by systems and behaviors through
//! [`SimulationContext::events`](super::SimulationContext::events) during the next update. Each reader therefore sees
//! each event once, in the order it was sent.
//!
//! Events are plain Rust values keyed by type: a game defines `struct EnemyDefeated { .. }`, sends it, and reads
//! `events.read::<EnemyDefeated>()`. No event names are strings.

use std::any::{
    Any,
    TypeId,
};
use std::borrow::Cow;
use std::fmt;

use indexmap::IndexMap;
use joy_physics::{
    BodyHandle,
    ContactPhase,
};

use super::entity::EntityId;
use super::world_timers::TimerId;

// ---------------------------------------------------------------------------------------------------------------------
// Engine Events
// ---------------------------------------------------------------------------------------------------------------------

/// Two bodies began or stopped touching (or a body entered or left a sensor) during the physics step.
///
/// `first` and `second` name the entities that own the bodies, or `None` for bodies no entity owns (level geometry a
/// game created directly, for example). Only pairs where a body reports contacts appear; see
/// `BodyOptions::report_contacts` and the `physics` component's `reportContacts` field.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactEvent {
    /// Began or ended.
    pub phase: ContactPhase,
    /// The entity owning `first_body`, if any.
    pub first: Option<EntityId>,
    /// The entity owning `second_body`, if any.
    pub second: Option<EntityId>,
    /// One body of the pair.
    pub first_body: BodyHandle,
    /// The other body of the pair.
    pub second_body: BodyHandle,
    /// Whether either body is a sensor.
    pub sensor: bool,
}

impl ContactEvent {
    /// Whether `entity` is one side of the pair.
    pub fn involves(&self, entity: &EntityId) -> bool {
        self.first.as_ref() == Some(entity) || self.second.as_ref() == Some(entity)
    }

    /// The other side of the pair when `entity` is one side; `Some(None)` when the other body has no entity.
    pub fn other(&self, entity: &EntityId) -> Option<Option<&EntityId>> {
        if self.first.as_ref() == Some(entity) {
            return Some(self.second.as_ref());
        }
        if self.second.as_ref() == Some(entity) {
            return Some(self.first.as_ref());
        }
        None
    }
}

/// A timer started through [`SimulationContext::start_timer`](super::SimulationContext::start_timer) or
/// [`World::start_timer`](super::World::start_timer) came due at the start of an update. Like every event, the game
/// reads it after that update and behaviors and systems read it during the next one.
#[derive(Clone, Debug, PartialEq)]
pub struct TimerFired {
    /// The timer that fired; a repeating timer keeps its id.
    pub timer: TimerId,
    /// The entity the timer belongs to, if any.
    pub owner: Option<EntityId>,
    /// The game's name for the timer (a constant, such as `const RELOAD: &str = "reload";`, or a script's text).
    pub key: Cow<'static, str>,
}

/// An entity joined the world (authored, spawned by the game, or requested by a behavior).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntitySpawned {
    /// The new entity.
    pub id: EntityId,
}

/// An entity left the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityRemoved {
    /// The removed entity; its id may be reused by a later spawn.
    pub id: EntityId,
}

// ---------------------------------------------------------------------------------------------------------------------
// Queues
// ---------------------------------------------------------------------------------------------------------------------

/// Events of any number of types, each kept in send order.
#[derive(Default)]
pub struct EventQueue {
    // One `Vec<T>` per event type, in the order types were first sent.
    queues: IndexMap<TypeId, Box<dyn ErasedEvents>>,
}

impl EventQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an event of type `T`.
    pub fn send<T: 'static>(&mut self, event: T) {
        let queue = self.queues.entry(TypeId::of::<T>()).or_insert_with(|| Box::new(Vec::<T>::new())).as_any_mut();
        if let Some(events) = queue.downcast_mut::<Vec<T>>() {
            events.push(event);
        }
    }

    /// Every event of type `T`, in send order.
    pub fn read<T: 'static>(&self) -> &[T] {
        self.queues
            .get(&TypeId::of::<T>())
            .and_then(|queue| queue.as_any().downcast_ref::<Vec<T>>())
            .map_or(&[], Vec::as_slice)
    }

    /// Total events of every type.
    pub fn len(&self) -> usize {
        self.queues.values().map(|queue| queue.len()).sum()
    }

    /// Whether no events are queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove every event, keeping each type's allocation for reuse.
    pub fn clear(&mut self) {
        for queue in self.queues.values_mut() {
            queue.clear();
        }
    }
}

impl fmt::Debug for EventQueue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("EventQueue").field("events", &self.len()).field("types", &self.queues.len()).finish()
    }
}

/// The events delivered to the current update (from the previous one) and the events the current update produces.
#[derive(Debug, Default)]
pub struct WorldEvents {
    delivered: EventQueue,
    produced: EventQueue,
}

impl WorldEvents {
    /// Events produced during the previous update: what systems and behaviors read now.
    pub fn delivered(&self) -> &EventQueue {
        &self.delivered
    }

    /// Events produced so far by the current update, or by the latest one once it returned.
    pub fn produced(&self) -> &EventQueue {
        &self.produced
    }

    /// Send an event into the current update's output.
    pub fn send<T: 'static>(&mut self, event: T) {
        self.produced.send(event);
    }

    /// Start an update boundary: the latest update's output becomes the delivered set, and the old delivered set is
    /// discarded.
    pub(crate) fn advance(&mut self) {
        std::mem::swap(&mut self.delivered, &mut self.produced);
        self.produced.clear();
    }

    /// Drop every event, for example when a world restarts.
    pub fn clear(&mut self) {
        self.delivered.clear();
        self.produced.clear();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Type Erasure
// ---------------------------------------------------------------------------------------------------------------------

/// A `Vec<T>` of events whose element type is known only to its sender and readers.
trait ErasedEvents: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn len(&self) -> usize;
    fn clear(&mut self);
}

impl<T: 'static> ErasedEvents for Vec<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn clear(&mut self) {
        Vec::clear(self);
    }
}
