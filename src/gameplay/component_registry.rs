// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Trusted project factories keyed by authored component type. Authored type names never evaluate code.

use std::fmt;

use indexmap::IndexMap;

use super::component::EntityComponent;
use super::entity::EntityState;
use super::error::GameplayError;
use crate::core::JsonRecord;

/// Creates the runtime component for one authored record: `(authored_record, entity_state) -> component`.
///
/// The record and state are borrowed for the call; the returned component is owned by its entity. Components read the
/// live record later through `entity.components` in their hooks.
pub type ComponentFactory = dyn Fn(&JsonRecord, &EntityState) -> Box<dyn EntityComponent>;

/// Maps authored component types to runtime component factories.
///
/// Registries start empty; authored component types without a factory stay plain data. Duplicate and blank types
/// fail, as in the TypeScript engine.
#[derive(Default)]
pub struct ComponentRegistry {
    factories: IndexMap<String, Box<ComponentFactory>>,
}

impl ComponentRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register project behavior for an authored component type.
    ///
    /// Fails for a blank type or a type that is already registered. The factory is called once per attachment.
    pub fn register(
        &mut self,
        kind: &str,
        factory: impl Fn(&JsonRecord, &EntityState) -> Box<dyn EntityComponent> + 'static,
    ) -> Result<&mut Self, GameplayError> {
        if kind.trim().is_empty() {
            return Err(GameplayError::EmptyComponentType);
        }
        if self.factories.contains_key(kind) {
            return Err(GameplayError::DuplicateComponentType(kind.to_owned()));
        }
        self.factories.insert(kind.to_owned(), Box::new(factory));
        Ok(self)
    }

    /// Whether this registry can create runtime behavior for an authored type.
    pub fn contains(&self, kind: &str) -> bool {
        self.factories.contains_key(kind)
    }

    /// Registered types in registration order.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.factories.keys().map(String::as_str)
    }

    /// Create an unattached runtime component over an entity's authored record.
    ///
    /// Fails with [`GameplayError::UnknownComponentType`] for an unregistered type.
    pub fn create(
        &self,
        kind: &str,
        data: &JsonRecord,
        entity: &EntityState,
    ) -> Result<Box<dyn EntityComponent>, GameplayError> {
        let factory = self.factories.get(kind).ok_or_else(|| GameplayError::UnknownComponentType(kind.to_owned()))?;
        Ok(factory(data, entity))
    }
}

impl fmt::Debug for ComponentRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ComponentRegistry").field("kinds", &self.factories.keys().collect::<Vec<_>>()).finish()
    }
}
