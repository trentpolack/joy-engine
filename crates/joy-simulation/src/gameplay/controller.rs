// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Controllers that turn sampled input or AI decisions into control intent for the entity they possess.
//!
//! [`IntentController`] covers both the TypeScript `PlayerController` (sampling borrowed keyboard, touch, or gamepad
//! adapters) and `AIController` (sampling decisions): both use the same intent and possession contract. Update
//! controllers before the entity world so behaviors see the new intent in the same frame.

use std::fmt;

use super::entity::{ControlIntent, ControllerId, EntityId, EntityState};
use super::entity_world::EntityWorld;
use super::error::GameplayError;

/// Caller name used in delta-time errors.
const CONTROLLER_DELTA: &str = "Controller delta";

/// Samples intent for the possessed entity: `(elapsed_seconds, possessed_state) -> intent`.
pub type IntentSampler = dyn FnMut(f32, &EntityState) -> ControlIntent;

/// A controller that owns its possession id and a sampler, and borrows the world on each call.
pub struct IntentController {
    id: ControllerId,
    sampler: Box<IntentSampler>,
}

impl IntentController {
    /// Create an unpossessing controller in `world` with a sampler.
    pub fn new(world: &mut EntityWorld, sampler: impl FnMut(f32, &EntityState) -> ControlIntent + 'static) -> Self {
        Self { id: world.create_controller(), sampler: Box::new(sampler) }
    }

    /// This controller's possession id.
    pub fn id(&self) -> ControllerId {
        self.id
    }

    /// Replace the sampler, for example when an input device reconnects.
    pub fn set_sampler(&mut self, sampler: impl FnMut(f32, &EntityState) -> ControlIntent + 'static) {
        self.sampler = Box::new(sampler);
    }

    /// Possess an entity, or release possession with `None`. See [`EntityWorld::possess`].
    pub fn possess(&self, world: &mut EntityWorld, target: Option<&EntityId>) -> Result<(), GameplayError> {
        world.possess(self.id, target)
    }

    /// The entity this controller possesses, if any.
    pub fn possessed<'a>(&self, world: &'a EntityWorld) -> Option<&'a EntityId> {
        world.possessed(self.id)
    }

    /// Sample and deliver intent for one frame of `elapsed_seconds` (finite, nonnegative).
    ///
    /// An invalid delta clears the possessed entity's intent and fails. Otherwise, when possessing, the entity's intent
    /// is cleared (with `reset_control`), the sampler runs, and the sample is delivered only if this controller still
    /// possesses the entity. A sample with a non-finite number fails and leaves the intent empty. Does nothing when not
    /// possessing or after the controller was released.
    pub fn update(&mut self, world: &mut EntityWorld, elapsed_seconds: f32) -> Result<(), GameplayError> {
        let target = world.possessed(self.id).cloned();
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            if let Some(target) = &target {
                world.reset_control(target);
            }
            return Err(GameplayError::InvalidDelta(CONTROLLER_DELTA));
        }
        let Some(target) = target else {
            return Ok(());
        };

        world.reset_control(&target);
        let Some(entity) = world.get(&target) else {
            return Ok(());
        };
        let intent = (self.sampler)(elapsed_seconds, &entity.state);

        // Never deliver a sample to an entity this controller no longer possesses.
        if world.possessed(self.id) != Some(&target) {
            return Ok(());
        }
        world.receive_control(&target, intent)
    }

    /// Release possession and retire this controller's id in `world`.
    pub fn release(self, world: &mut EntityWorld) {
        world.release_controller(self.id);
    }
}

impl fmt::Debug for IntentController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("IntentController").field("id", &self.id).finish()
    }
}
