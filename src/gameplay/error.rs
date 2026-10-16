// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Typed failures for authored documents and runtime gameplay.

use crate::core::PostProcessingError;

use super::entity::EntityId;
use crate::physics::PhysicsError;

/// An authored asset reference was unsafe or had the wrong extension.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AssetPathError {
    /// The path did not start with `assets/`, contained `\ ? # % :` or control characters, or had empty, `.` or `..`
    /// segments.
    #[error("Asset path must stay beneath assets/.")]
    OutsideAssets,
    /// The path did not end with any allowed extension; the payload lists them joined by " or ".
    #[error("Asset path must use {0}.")]
    WrongExtension(String),
}

/// An entity definition failed validation.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DefinitionError {
    /// The definition was not a JSON object.
    #[error("Entity definition must be an object.")]
    NotAnObject,
    /// The id was missing, not a string, or blank.
    #[error("Entity id must be a nonempty string.")]
    InvalidId,
    /// The name was not a string, or the type was not a nonblank string.
    #[error("Entity name and type must be strings; type cannot be empty.")]
    InvalidNameOrType,
    /// Tags were not an array of strings.
    #[error("Entity tags must be strings.")]
    InvalidTags,
    /// The transform or components value was not an object.
    #[error("Entity transform and components must be records.")]
    InvalidRecords,
    /// A component value was not a JSON object.
    #[error("Component {0} must be a JSON record.")]
    InvalidComponent(String),
    /// A transform vector did not contain exactly three finite numbers (representable as `f32`).
    #[error("Entity {0} must contain three finite numbers.")]
    InvalidVector(&'static str),
    /// The template path was invalid.
    #[error(transparent)]
    AssetPath(#[from] AssetPathError),
    /// Overrides appeared without a template, or were not an object.
    #[error("Object overrides require a template and a JSON record.")]
    InvalidOverrides,
    /// Overrides contained keys other than `type`, `tags`, and `components`.
    #[error("Object overrides support type, tags and components only.")]
    UnsupportedOverride,
}

/// A level document failed to parse or validate.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum LevelError {
    /// The text was not valid JSON.
    #[error("Invalid level JSON: {0}")]
    Json(String),
    /// The document was not a JSON object.
    #[error("A level must be an object.")]
    NotAnObject,
    /// The version was not 1.
    #[error("Unsupported level version; expected version 1.")]
    UnsupportedVersion,
    /// The name was missing or blank.
    #[error("A level requires a name.")]
    MissingName,
    /// `entities` was missing or not an array.
    #[error("A level requires an entities array.")]
    MissingEntities,
    /// Two entities shared an id.
    #[error("Duplicate entity ID: {0}")]
    DuplicateEntityId(String),
    /// An entity definition was invalid.
    #[error(transparent)]
    Definition(#[from] DefinitionError),
    /// The post-processing profile was invalid.
    #[error(transparent)]
    PostProcessing(#[from] PostProcessingError),
}

/// A reusable object document failed to parse, validate, or resolve.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ObjectError {
    /// The text was not valid JSON.
    #[error("Invalid object JSON: {0}")]
    Json(String),
    /// The document lacked version 1, a nonblank name, or an entity.
    #[error("An object requires version 1, a name and an entity.")]
    InvalidObject,
    /// The object's entity itself linked a template or carried overrides.
    #[error("Object definitions cannot contain nested templates or overrides.")]
    NestedTemplate,
    /// An entity definition (source or resolved instance) was invalid.
    #[error(transparent)]
    Definition(#[from] DefinitionError),
    /// The level being resolved was invalid.
    #[error(transparent)]
    Level(#[from] LevelError),
    /// A template path was invalid.
    #[error(transparent)]
    AssetPath(#[from] AssetPathError),
    /// The caller's reader failed for a path.
    #[error("Could not read {path}: {message}")]
    Read {
        /// The validated asset path that was requested.
        path: String,
        /// The reader's error message.
        message: String,
    },
}

/// An authored presentation component could not be interpreted.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ComponentError {
    /// A light scalar was present but not a finite number.
    #[error("Scene light values must be finite numbers.")]
    NonFiniteLightValue,
    /// Light color or intensity was negative, or range was not positive.
    #[error("Light color/intensity must be nonnegative and range must be positive.")]
    InvalidLight,
    /// Asset parameters were present but not an object.
    #[error("Asset parameters must be a record.")]
    InvalidParameters,
    /// A mesh asset path was invalid.
    #[error(transparent)]
    AssetPath(#[from] AssetPathError),
}

/// A component schema could not be registered, or a preset was unknown. Messages match the TypeScript engine.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ComponentSchemaError {
    /// The schema's type was blank.
    #[error("Component schemas require a nonempty type.")]
    EmptyType,
    /// The schema offered no presets.
    #[error("Component schema {0} requires at least one preset.")]
    NoPresets(String),
    /// A field name was blank or repeated within one field list.
    #[error("Component schema {0} requires unique, nonempty field names.")]
    InvalidFieldNames(String),
    /// A `choice` field listed no choices.
    #[error("Choice field {kind}.{field} requires choices.")]
    MissingChoices {
        /// Component type.
        kind: String,
        /// Field name.
        field: String,
    },
    /// An `asset` field listed no asset kinds.
    #[error("Asset field {kind}.{field} requires asset kinds.")]
    MissingAssetKinds {
        /// Component type.
        kind: String,
        /// Field name.
        field: String,
    },
    /// A field other than a `list` declared item fields.
    #[error("Only list fields may declare items ({kind}.{field}).")]
    ItemsOnNonList {
        /// Component type.
        kind: String,
        /// Field name.
        field: String,
    },
    /// A field's condition did not name a sibling `choice` field.
    #[error("Component field {kind}.{field} must depend on a sibling choice field.")]
    InvalidCondition {
        /// Component type.
        kind: String,
        /// Field name.
        field: String,
    },
    /// A preset id was blank.
    #[error("Component schema {0} presets require an id, label and record data.")]
    InvalidPreset(String),
    /// One schema listed a preset id twice.
    #[error("Component schema {kind} repeats preset {preset}.")]
    RepeatedPreset {
        /// Component type.
        kind: String,
        /// Repeated preset id.
        preset: String,
    },
    /// A schema for this type was already registered.
    #[error("Component schema {0} is already registered.")]
    DuplicateSchema(String),
    /// Another schema already registered this preset id.
    #[error("Component preset {0} is already registered.")]
    DuplicatePreset(String),
    /// No registered schema offers this preset id.
    #[error("Unknown component preset: {0}.")]
    UnknownPreset(String),
}

/// A `physics` or `vehicle` component could not become solver objects.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PhysicsComponentError {
    /// A `vehicle` component appeared without a `physics` component.
    #[error("vehicle requires a physics component.")]
    VehicleWithoutPhysics,
    /// The entity scale had a zero or negative component.
    #[error("Physics entity scale must be positive.")]
    NonPositiveScale,
    /// `shape` was not `box`, `sphere`, or `plane`.
    #[error("Physics shape must be box, sphere or plane.")]
    InvalidShape,
    /// Box half extents were not three finite positive numbers.
    #[error("Box halfExtents require three finite positive numbers.")]
    InvalidHalfExtents,
    /// A sphere collider was placed on a nonuniformly scaled entity.
    #[error("Sphere physics requires uniform entity scale.")]
    NonUniformSphereScale,
    /// The sphere radius was not finite and positive.
    #[error("Sphere radius must be finite and positive.")]
    InvalidRadius,
    /// A vehicle was placed on a nonuniformly scaled entity.
    #[error("Vehicle physics requires uniform entity scale.")]
    NonUniformVehicleScale,
    /// `wheels` was missing or not an array.
    #[error("Vehicle wheels must be an array.")]
    InvalidWheels,
    /// A wheel entry was not an object.
    #[error("wheel component must be an object.")]
    InvalidWheel,
    /// A wheel position was not three finite numbers.
    #[error("Wheel position requires three finite numbers.")]
    InvalidWheelPosition,
    /// A wheel radius, rest length, or travel was negative or not a finite number.
    #[error("Wheel dimensions must be finite and nonnegative.")]
    InvalidWheelDimensions,
    /// A wheel flag (`steering`, `driven`, `brake`) was present but not a boolean.
    #[error("Wheel {0} must be boolean.")]
    InvalidWheelFlag(&'static str),
    /// A numeric option was present but not a number.
    #[error("{0} must be a finite number.")]
    NotANumber(&'static str),
    /// The solver rejected the resulting configuration.
    #[error(transparent)]
    Physics(#[from] PhysicsError),
}

/// Runtime gameplay failure.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GameplayError {
    /// An entity with this id is already live.
    #[error("Entity id {0} is already present.")]
    DuplicateEntity(EntityId),
    /// No factory is registered for the definition's type.
    #[error("Unknown entity type: {0}.")]
    UnknownEntityType(String),
    /// A factory was registered twice for one type.
    #[error("Entity type {0} is already registered.")]
    DuplicateEntityType(String),
    /// A factory was registered for a blank type.
    #[error("Entity registration requires a nonempty type and factory.")]
    EmptyEntityType,
    /// The entity is not live in this world.
    #[error("Entity {0} is not present.")]
    UnknownEntity(EntityId),
    /// Possession targeted an entity that is not live.
    #[error("Cannot possess a destroyed or invalid character.")]
    InvalidPossessionTarget,
    /// The controller was released (or never created by this world).
    #[error("Cannot possess with a destroyed controller.")]
    UnknownController,
    /// A delta time was negative or non-finite; the payload names the caller (for example "World delta").
    #[error("{0} must be finite nonnegative seconds.")]
    InvalidDelta(&'static str),
    /// A control intent contained a non-finite number (or, from JSON, a value that was not a number or boolean).
    #[error("Control intent values must be finite numbers or booleans.")]
    NonFiniteIntent,
    /// A JSON control intent was not an object.
    #[error("Control intent must be a record.")]
    InvalidIntentRecord,
    /// A component factory was registered (or a component attached) under a blank type.
    #[error("Component registration requires a nonempty type and factory.")]
    EmptyComponentType,
    /// A component factory was registered twice for one type.
    #[error("Component type {0} is already registered.")]
    DuplicateComponentType(String),
    /// No component factory is registered for the type.
    #[error("Unknown component type: {0}.")]
    UnknownComponentType(String),
    /// The entity already carries a runtime component of this type.
    #[error("Entity {entity} already has component {kind}.")]
    DuplicateComponent {
        /// The entity that already carries the component.
        entity: EntityId,
        /// The component type.
        kind: String,
    },
    /// A system listed a blank required component type.
    #[error("System component requirements must be nonempty strings.")]
    InvalidSystemRequirement,
    /// A level still linked object templates.
    #[error("Entity {0}: resolve linked objects with resolve_level_objects before creating LevelRuntime.")]
    UnresolvedTemplate(EntityId),
    /// A physics or vehicle component could not be built.
    #[error("Entity {entity}: {source}")]
    PhysicsComponent {
        /// The entity whose component failed.
        entity: EntityId,
        /// What was wrong with the component.
        source: PhysicsComponentError,
    },
    /// A project behavior reported a failure (for example from `on_spawn`).
    #[error("{0}")]
    Behavior(String),
    /// A definition was invalid.
    #[error(transparent)]
    Definition(#[from] DefinitionError),
    /// A level was invalid.
    #[error(transparent)]
    Level(#[from] LevelError),
    /// The physics world rejected an operation.
    #[error(transparent)]
    Physics(#[from] PhysicsError),
}
