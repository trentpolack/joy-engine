// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! World-owned processors for entities selected by runtime component composition.

use super::entity::{
    Entity,
    SimulationContext,
};
use super::error::GameplayError;

/// Identifier of a system added with [`EntityWorld::add_system`](super::EntityWorld::add_system).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SystemId(pub(crate) u64);

/// World-owned processor for entities selected by runtime component composition.
///
/// Systems run before entity behaviors in every update, in the order they were added, and receive a stable
/// insertion-order list of the frame's surviving entities that carry every required runtime component (attached by the
/// world's [`ComponentRegistry`](super::ComponentRegistry) or [`EntityWorld::add_component`]). Lifetime hooks follow
/// the TypeScript engine: [`EntitySystem::on_add`] runs when the world takes ownership and [`EntitySystem::on_remove`]
/// exactly once when the system is removed or the world shuts down.
///
/// [`EntityWorld::add_component`]: super::EntityWorld::add_component
pub trait EntitySystem {
    /// Runtime component types an entity must carry to be passed to [`EntitySystem::update`]; empty matches every
    /// entity. Types must be nonblank; duplicates are harmless.
    fn required_components(&self) -> &[&str] {
        &[]
    }

    /// Acquire system-wide resources after the world takes ownership. Returning an error removes the system again
    /// (calling [`EntitySystem::on_remove`]) and fails `add_system`.
    fn on_add(&mut self) -> Result<(), GameplayError> {
        Ok(())
    }

    /// Process matching entities by finite, nonnegative simulation seconds. Structural changes go through `context`.
    fn update(&mut self, elapsed_seconds: f32, matches: &mut [&mut Entity], context: &mut SimulationContext<'_>);

    /// Release system-wide resources; called exactly once.
    fn on_remove(&mut self) {}
}
