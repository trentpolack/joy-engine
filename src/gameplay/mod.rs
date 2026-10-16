// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Entities, runtime components, systems, controllers, authored documents, schemas, and level runtimes.
//!
//! Data flow: a `.joylevel` document parses into [`LevelData`] (validated [`EntityDefinition`] records), linked
//! `.joyobject` templates resolve through [`resolve_level_objects`], and [`LevelRuntime`] instantiates the level into a
//! [`World`]: an [`EntityWorld`] of live entities plus a [`PhysicsWorld`](crate::physics::PhysicsWorld) holding the
//! bodies and vehicles their `physics` and `vehicle` components describe. Joy Editor edits levels through
//! [`LevelDocument`] (atomic transactions with undo and redo) and validates component records against the
//! [`ComponentSchemaCatalog`].
//!
//! Ownership: the world owns entities and hands out [`EntityId`] and [`ControllerId`] values. Project logic lives in
//! [`EntityBehavior`] implementations created by [`EntityRegistry`] factories and in [`EntityComponent`]s created by
//! [`ComponentRegistry`] factories for authored component types; hooks receive the entity's [`EntityState`] and a
//! [`SimulationContext`] (split borrows of physics and the handle maps), never references to other entities.
//! [`EntitySystem`]s process entities selected by runtime components. [`IntentController`] samples player input or AI
//! decisions into each possessed entity's [`ControlIntent`]; [`Controller`] only holds possession.

mod asset_path;
mod built_in_component_schemas;
mod character;
mod component;
mod component_registry;
mod component_schema;
mod components;
mod controller;
mod entity;
mod entity_definition;
mod entity_registry;
mod entity_system;
mod entity_world;
mod error;
pub mod grid_navigation;
mod level_document;
mod level_runtime;
mod object_document;
mod player_controller;
mod world;

pub use asset_path::asset_path;
pub use built_in_component_schemas::built_in_component_schemas;
pub use character::{ControlIntent, ControlValue};
pub use component::EntityComponent;
pub use component_registry::{ComponentFactory, ComponentRegistry};
pub use component_schema::{
    ComponentFieldCondition, ComponentFieldKind, ComponentFieldSchema, ComponentIssue, ComponentPreset,
    ComponentSchema, ComponentSchemaCatalog, CreatedComponent, is_component_field_active,
};
pub use components::{
    EFFECT_COMPONENT_DEFAULTS, EffectComponentDefaults, LIGHT_COMPONENT_DEFAULTS, LightComponent,
    LightComponentDefaults, RENDER_COMPONENT_DEFAULTS, RenderComponent, RenderComponentDefaults, RenderSource,
    component_asset_kind, component_category, component_preset, component_type, entity_type,
};
pub use controller::Controller;
pub use entity::{ControllerId, Entity, EntityBehavior, EntityId, EntityState, SimulationContext};
pub use entity_definition::{EntityDefinition, EntityTransform};
pub use entity_registry::EntityRegistry;
pub use entity_system::{EntitySystem, SystemId};
pub use entity_world::EntityWorld;
pub use error::{
    AssetPathError, ComponentError, ComponentSchemaError, DefinitionError, GameplayError, LevelError, ObjectError,
    PhysicsComponentError,
};
pub use grid_navigation::{
    DEFAULT_CELL_LIMIT, GridRoute, GridRoutes, GridSearchError, GridSearchOptions, search_grid, trace_grid_route,
};
pub use level_document::{LEVEL_HISTORY_LIMIT, LEVEL_VERSION, LevelData, LevelDocument, parse_level, serialize_level};
pub use level_runtime::LevelRuntime;
pub use object_document::{
    OBJECT_VERSION, ObjectDocument, create_object_instance, parse_object, resolve_level_objects,
    resolve_object_instance, serialize_object,
};
pub use player_controller::{AiController, FallibleIntentSampler, IntentController, IntentSampler, PlayerController};
pub use world::{World, WorldStep};
