// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime behavior attached to one entity per authored component type.
//!
//! Authored component data stays a plain JSON record in [`EntityState::components`]; a runtime component is the
//! project logic and resources a [`ComponentRegistry`](super::ComponentRegistry) factory creates for that record.
//! Components compose behavior without replacing the entity's [`EntityBehavior`](super::EntityBehavior), and
//! [`EntitySystem`](super::EntitySystem)s and [`EntityWorld::query`](super::EntityWorld::query) select entities by the
//! runtime components they carry.
//!
//! Lifecycle:
//! - Spawning attaches registered components in authored component order, before the behavior's `on_spawn`.
//! - [`EntityComponent::on_attach`] acquires resources; returning an error detaches the component (running
//!   [`EntityComponent::on_detach`]) and rolls back the whole spawn.
//! - Removal runs the behavior's `on_destroy`, then detaches components in reverse attachment order: each component's
//!   `on_detach` runs exactly once and the component is dropped, releasing its fields in declaration order before
//!   the entity's own resources.

use std::any::Any;

use super::entity::{
    EntityState,
    SimulationContext,
};
use super::error::GameplayError;

/// Project logic and resources for one component type on one entity.
///
/// The component's type is the key it was registered and attached under; its authored record is
/// `entity.components[type]`, which hooks may read and edit. Hooks receive the owning entity's state and the
/// simulation context, never references to other entities. Downcast attached components with
/// [`Entity::component`](super::Entity::component).
pub trait EntityComponent: Any {
    /// Acquire resources after attachment. Returning an error detaches the component (calling
    /// [`EntityComponent::on_detach`]) and, during spawn, rolls back the entity.
    fn on_attach(
        &mut self,
        entity: &mut EntityState,
        context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        let _ = (entity, context);
        Ok(())
    }

    /// Release component state; called exactly once when the component is detached, including after a failed
    /// [`EntityComponent::on_attach`]. Owned Rust resources are released afterwards by `Drop`.
    fn on_detach(&mut self, entity: &mut EntityState, context: &mut SimulationContext<'_>) {
        let _ = (entity, context);
    }
}

/// Downcast a boxed component to its concrete type.
pub(crate) fn downcast_component<T: EntityComponent>(component: &dyn EntityComponent) -> Option<&T> {
    let any: &dyn Any = component;
    any.downcast_ref::<T>()
}

/// Downcast a boxed component to its concrete type, mutably.
pub(crate) fn downcast_component_mut<T: EntityComponent>(component: &mut dyn EntityComponent) -> Option<&mut T> {
    let any: &mut dyn Any = component;
    any.downcast_mut::<T>()
}
