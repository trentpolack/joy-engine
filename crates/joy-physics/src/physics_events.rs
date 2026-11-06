// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Collision groups and the contact and sensor events a world reports after each step.
//!
//! Bodies opt into events: sensors always report, and solid bodies report when created with
//! [`BodyOptions::report_contacts`](super::BodyOptions::report_contacts). A pair reports when either body opts in.
//! Events are collected across every fixed step of one [`PhysicsWorld::step`](super::PhysicsWorld::step) call, in the
//! solver's deterministic order, and replaced by the next call.

use std::sync::Mutex;

use rapier3d::dynamics::RigidBodySet;
use rapier3d::geometry::{
    ColliderSet,
    CollisionEvent,
    ContactPair,
    Group,
    InteractionGroups,
    InteractionTestMode,
};
use rapier3d::pipeline::EventHandler;

use super::BodyHandle;

// ---------------------------------------------------------------------------------------------------------------------
// Collision Groups
// ---------------------------------------------------------------------------------------------------------------------

/// Collision layers as bit masks: the layers a body belongs to and the layers it interacts with.
///
/// Two bodies interact (collide, report sensor overlaps, and find each other in filtered queries) only when each one's
/// `membership` intersects the other's `filter`. Games name their layers with constants, for example
/// `const PLAYER: u32 = 1 << 0;`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CollisionGroups {
    /// Layers this body belongs to.
    pub membership: u32,
    /// Layers this body interacts with.
    pub filter: u32,
}

impl CollisionGroups {
    /// Member of every layer and interacting with every layer: the default for bodies and queries.
    pub const ALL: Self = Self::new(u32::MAX, u32::MAX);

    /// Groups from explicit membership and filter masks.
    pub const fn new(membership: u32, filter: u32) -> Self {
        Self {
            membership,
            filter,
        }
    }

    /// Whether two group pairs interact: each membership must intersect the other's filter.
    pub const fn interacts_with(self, other: Self) -> bool {
        ((self.membership & other.filter) != 0) && ((other.membership & self.filter) != 0)
    }

    /// The solver's representation of these groups.
    pub(super) fn to_solver(self) -> InteractionGroups {
        InteractionGroups::new(
            Group::from_bits_retain(self.membership),
            Group::from_bits_retain(self.filter),
            InteractionTestMode::And,
        )
    }

    /// Groups read back from the solver's representation.
    pub(super) fn from_solver(groups: InteractionGroups) -> Self {
        Self::new(groups.memberships.bits(), groups.filter.bits())
    }
}

impl Default for CollisionGroups {
    fn default() -> Self {
        Self::ALL
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------------------------------------------------

/// Whether a pair of bodies began or stopped touching.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContactPhase {
    /// The bodies began touching (or a body entered a sensor) during the step.
    Started,
    /// The bodies stopped touching (or a body left a sensor) during the step, including because one was removed.
    Ended,
}

/// One change in contact between two bodies during a [`PhysicsWorld::step`](super::PhysicsWorld::step) call.
///
/// `first` and `second` follow the solver's pair order; neither is privileged. A handle may name a body removed since
/// the contact began, so consumers mapping handles to their own records should tolerate unknown ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicsEvent {
    /// Began or ended.
    pub phase: ContactPhase,
    /// One body of the pair.
    pub first: BodyHandle,
    /// The other body of the pair.
    pub second: BodyHandle,
    /// Whether either body is a sensor (an overlap with no collision response).
    pub sensor: bool,
}

impl PhysicsEvent {
    /// The other body of the pair, when `body` is one of them.
    pub fn other(&self, body: BodyHandle) -> Option<BodyHandle> {
        if self.first == body {
            return Some(self.second);
        }
        if self.second == body {
            return Some(self.first);
        }
        None
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Solver Collector
// ---------------------------------------------------------------------------------------------------------------------

/// Receives the solver's collision events during one fixed step.
///
/// The solver hands events to a shared reference, so they are buffered behind a mutex; the world drains them into
/// [`PhysicsEvent`]s after the step, when collider handles can still be resolved to bodies.
#[derive(Default)]
pub(super) struct EventCollector {
    collisions: Mutex<Vec<CollisionEvent>>,
}

impl EventCollector {
    /// Take every buffered event, in arrival order.
    pub(super) fn take(&self) -> Vec<CollisionEvent> {
        match self.collisions.lock() {
            Ok(mut events) => std::mem::take(&mut *events),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }
}

impl EventHandler for EventCollector {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        event: CollisionEvent,
        _contact_pair: Option<&ContactPair>,
    ) {
        match self.collisions.lock() {
            Ok(mut events) => events.push(event),
            Err(poisoned) => poisoned.into_inner().push(event),
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: f32,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _contact_pair: &ContactPair,
        _total_force_magnitude: f32,
    ) {
    }

    fn handle_soft_body_tear_event(
        &self,
        _soft_bodies: &rapier3d::dynamics::SoftBodySet,
        _event: &rapier3d::dynamics::SoftBodyTearEvent,
    ) {
    }
}
