// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The possession-only controller (TypeScript `controller.ts`).
//!
//! A [`Controller`] owns exclusive possession of one entity but samples nothing: intent arrives from elsewhere through
//! [`EntityWorld::receive_control`] (network peers, replays, scripted sequences). Sampling controllers are
//! [`IntentController`](super::IntentController)s. Update controllers before the entity world so behaviors see the new
//! intent in the same frame.

use super::entity::{ControllerId, EntityId};
use super::entity_world::EntityWorld;
use super::error::GameplayError;

/// Caller name used in delta-time errors.
const CONTROLLER_DELTA: &str = "Controller delta";

/// A controller that owns its possession id and borrows the world on each call.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Controller {
    id: ControllerId,
}

impl Controller {
    /// Create an unpossessing controller in `world`.
    pub fn new(world: &mut EntityWorld) -> Self {
        Self { id: world.create_controller() }
    }

    /// This controller's possession id.
    pub fn id(&self) -> ControllerId {
        self.id
    }

    /// Possess an entity, or release possession with `None`. See [`EntityWorld::possess`].
    pub fn possess(&self, world: &mut EntityWorld, target: Option<&EntityId>) -> Result<(), GameplayError> {
        world.possess(self.id, target)
    }

    /// The entity this controller possesses, if any.
    pub fn possessed<'a>(&self, world: &'a EntityWorld) -> Option<&'a EntityId> {
        world.possessed(self.id)
    }

    /// Validate one frame of `elapsed_seconds`; the base controller samples nothing.
    ///
    /// A negative or non-finite delta clears the possessed entity's intent (calling `reset_control`) and fails, like
    /// the TypeScript `Controller.update`.
    pub fn update(&self, world: &mut EntityWorld, elapsed_seconds: f32) -> Result<(), GameplayError> {
        validate_controller_delta(world, self.id, elapsed_seconds)
    }

    /// Release possession and retire this controller's id in `world`. Releasing twice is harmless.
    pub fn release(self, world: &mut EntityWorld) {
        world.release_controller(self.id);
    }
}

/// Reset the possessed entity and fail for a negative or non-finite delta.
pub(crate) fn validate_controller_delta(
    world: &mut EntityWorld,
    controller: ControllerId,
    elapsed_seconds: f32,
) -> Result<(), GameplayError> {
    if elapsed_seconds.is_finite() && elapsed_seconds >= 0.0 {
        return Ok(());
    }
    if let Some(target) = world.possessed(controller).cloned() {
        world.reset_control(&target);
    }
    Err(GameplayError::InvalidDelta(CONTROLLER_DELTA))
}
