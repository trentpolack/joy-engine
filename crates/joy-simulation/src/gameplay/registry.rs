// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Trusted project factories keyed by authored entity type. Authored type names never evaluate code.

use std::fmt;

use indexmap::IndexMap;

use super::components::entity_type;
use super::definition::EntityDefinition;
use super::entity::EntityBehavior;
use super::error::GameplayError;

/// Creates the behavior for one spawned definition.
type BehaviorFactory = Box<dyn Fn(&EntityDefinition) -> Box<dyn EntityBehavior>>;

/// Maps entity types to behavior factories.
///
/// Like the TypeScript `EntityRegistry`, every registry starts with the built-in `entity`, `character`, and
/// `player-character` types (plain entities without behavior), duplicate registration fails, and spawning an
/// unregistered type fails with [`GameplayError::UnknownEntityType`].
pub struct EntityRegistry {
    // `None` marks a type that spawns a plain entity without behavior.
    factories: IndexMap<String, Option<BehaviorFactory>>,
}

impl EntityRegistry {
    /// A registry holding only the built-in types.
    pub fn new() -> Self {
        let mut factories = IndexMap::new();
        for kind in [entity_type::ENTITY, entity_type::CHARACTER, entity_type::PLAYER_CHARACTER] {
            factories.insert(kind.to_owned(), None);
        }
        Self { factories }
    }

    /// Register a trusted behavior factory for an entity type.
    ///
    /// Fails for a blank type or a type that is already registered (including the built-ins). The factory is called
    /// once per spawn with the validated, copied definition.
    pub fn register(
        &mut self,
        kind: &str,
        factory: impl Fn(&EntityDefinition) -> Box<dyn EntityBehavior> + 'static,
    ) -> Result<&mut Self, GameplayError> {
        self.insert(kind, Some(Box::new(factory)))
    }

    /// Register a type that spawns plain entities without behavior (for example scenery `prop`s).
    pub fn register_plain(&mut self, kind: &str) -> Result<&mut Self, GameplayError> {
        self.insert(kind, None)
    }

    /// Whether a type is registered.
    pub fn contains(&self, kind: &str) -> bool {
        self.factories.contains_key(kind)
    }

    /// Registered types in registration order.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.factories.keys().map(String::as_str)
    }

    /// Create the behavior for a definition: `Ok(None)` for plain types, an error for unregistered types.
    pub fn create(&self, definition: &EntityDefinition) -> Result<Option<Box<dyn EntityBehavior>>, GameplayError> {
        let factory = self
            .factories
            .get(&definition.kind)
            .ok_or_else(|| GameplayError::UnknownEntityType(definition.kind.clone()))?;
        Ok(factory.as_ref().map(|factory| factory(definition)))
    }

    /// Validate and store one registration.
    fn insert(&mut self, kind: &str, factory: Option<BehaviorFactory>) -> Result<&mut Self, GameplayError> {
        if kind.trim().is_empty() {
            return Err(GameplayError::EmptyEntityType);
        }
        if self.factories.contains_key(kind) {
            return Err(GameplayError::DuplicateEntityType(kind.to_owned()));
        }
        self.factories.insert(kind.to_owned(), factory);
        Ok(self)
    }
}

impl Default for EntityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for EntityRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("EntityRegistry").field("kinds", &self.factories.keys().collect::<Vec<_>>()).finish()
    }
}
