// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Entities, controllers, authored documents, and level runtimes.
//!
//! Data flow: a `.joylevel` document parses into [`LevelData`] (validated [`EntityDefinition`] records), linked
//! `.joyobject` templates resolve through [`resolve_level_objects`], and [`LevelRuntime`] instantiates the level into a
//! [`World`]: an [`EntityWorld`] of live entities plus a [`PhysicsWorld`](crate::physics::PhysicsWorld) holding the
//! bodies and vehicles their `physics` and `vehicle` components describe.
//!
//! Ownership: the world owns entities and hands out [`EntityId`] and [`ControllerId`] values. Project logic lives in
//! [`EntityBehavior`] implementations created by [`EntityRegistry`] factories; behaviors receive the entity's
//! [`EntityState`] and a [`SimulationContext`] (split borrows of physics and the handle maps), never references to
//! other entities. [`IntentController`] samples player input or AI decisions into each possessed entity's
//! [`ControlIntent`].

mod asset_path;
mod components;
mod controller;
mod definition;
mod entity;
mod entity_world;
mod error;
pub mod grid_navigation;
mod level_document;
mod level_runtime;
mod object_document;
mod registry;
mod world;

pub use asset_path::asset_path;
pub use components::{
    EFFECT_COMPONENT_DEFAULTS, EffectComponentDefaults, LIGHT_COMPONENT_DEFAULTS, LightComponent,
    LightComponentDefaults, RENDER_COMPONENT_DEFAULTS, RenderComponent, RenderComponentDefaults, RenderSource,
    component_type, entity_type,
};
pub use controller::{IntentController, IntentSampler};
pub use definition::{EntityDefinition, EntityTransform};
pub use entity::{
    ControlIntent, ControlValue, ControllerId, Entity, EntityBehavior, EntityId, EntityState, SimulationContext,
};
pub use entity_world::{EntitySystem, EntityWorld, SystemId};
pub use error::{
    AssetPathError, ComponentError, DefinitionError, GameplayError, LevelError, ObjectError, PhysicsComponentError,
};
pub use grid_navigation::{
    DEFAULT_CELL_LIMIT, GridRoute, GridRoutes, GridSearchError, GridSearchOptions, search_grid, trace_grid_route,
};
pub use level_document::{LEVEL_VERSION, LevelData, parse_level, serialize_level};
pub use level_runtime::LevelRuntime;
pub use object_document::{
    OBJECT_VERSION, ObjectDocument, create_object_instance, parse_object, resolve_level_objects,
    resolve_object_instance, serialize_object,
};
pub use registry::EntityRegistry;
pub use world::{World, WorldStep};
