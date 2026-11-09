// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The event records a world update produces itself. Queues, channels, and cursors are `joy_core::events`.
//!
//! An event lives for exactly one update boundary in the world's [`EventChannel`](joy_core::EventChannel). Everything
//! produced during an update (events behaviors and systems send, contacts from the physics step, timers that came due,
//! and entities spawned or removed) is readable by the game through [`World::events`](super::World::events) after that
//! update returns, and by systems and behaviors through
//! [`SimulationContext::events`](super::SimulationContext::events) during the next update. Presentation that reads once
//! per frame takes an [`EventCursor`](joy_core::EventCursor) over [`World::event_channel`](super::World::event_channel)
//! and sees every event once, however many updates the frame ran.
//!
//! Events are plain Rust values keyed by type: a game defines `struct EnemyDefeated { .. }`, sends it, and reads
//! `events.read::<EnemyDefeated>()`. No event names are strings.

use std::borrow::Cow;

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
