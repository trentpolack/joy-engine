// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Versioned `.joylevel` documents and the editable [`LevelDocument`] with undo history.
//!
//! Parsing validates the version, a nonblank name, every entity definition, and unique entity ids. Serialization writes
//! pretty JSON with two-space indentation, stable key order, and a trailing newline, preserving the authored document
//! format. Component data is never evaluated; post-processing lives in entities' `postProcess` components.

use std::collections::VecDeque;

use serde::de::Error as _;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
};
use serde_json::{
    Map,
    Value,
};

use super::entity_definition::EntityDefinition;
use super::error::LevelError;

/// The only supported level format version.
pub const LEVEL_VERSION: u32 = 1;

/// The retired level-wide post-processing key, rejected with a migration message.
const RETIRED_POST_PROCESSING_KEY: &str = "postProcessing";

/// Undo transactions a [`LevelDocument`] keeps; older snapshots are discarded first.
pub const LEVEL_HISTORY_LIMIT: usize = 80;

/// Owned, validated level data.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelData {
    /// Format version; always [`LEVEL_VERSION`].
    pub version: u32,
    /// Nonblank display name.
    pub name: String,
    /// Entity definitions in authored order; ids are unique.
    pub entities: Vec<EntityDefinition>,
}

impl LevelData {
    /// An empty level with a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: LEVEL_VERSION,
            name: name.into(),
            entities: Vec::new(),
        }
    }

    /// Validate a parsed JSON level and return an owned copy.
    pub fn from_json(value: &Value) -> Result<Self, LevelError> {
        let Value::Object(record) = value else {
            return Err(LevelError::NotAnObject);
        };
        if record.get("version").and_then(Value::as_f64) != Some(f64::from(LEVEL_VERSION)) {
            return Err(LevelError::UnsupportedVersion);
        }
        let name = match record.get("name") {
            Some(Value::String(name)) if !name.trim().is_empty() => name.clone(),
            _ => return Err(LevelError::MissingName),
        };
        let Some(Value::Array(authored)) = record.get("entities") else {
            return Err(LevelError::MissingEntities);
        };

        let mut entities: Vec<EntityDefinition> = Vec::with_capacity(authored.len());
        for value in authored {
            let entity = EntityDefinition::from_json(value)?;
            if entities.iter().any(|existing| existing.id == entity.id) {
                return Err(LevelError::DuplicateEntityId(entity.id));
            }
            entities.push(entity);
        }

        // Post-processing moved into `postProcess` volume components; a leftover level-wide profile would silently stop
        // applying, so it fails with a migration message instead.
        if record.contains_key(RETIRED_POST_PROCESSING_KEY) {
            return Err(LevelError::LevelPostProcessingMoved);
        }
        Ok(Self {
            version: LEVEL_VERSION,
            name,
            entities,
        })
    }

    /// Serialize in the format's key order: `version`, `name`, then `entities`.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        record.insert("version".to_owned(), Value::from(self.version));
        record.insert("name".to_owned(), Value::String(self.name.clone()));
        record
            .insert("entities".to_owned(), Value::Array(self.entities.iter().map(EntityDefinition::to_json).collect()));
        Value::Object(record)
    }

    /// Re-check a level built or edited in code with the same rules as [`parse_level`].
    pub fn validate(&self) -> Result<(), LevelError> {
        Self::from_json(&self.to_json()).map(|_| ())
    }
}

impl Serialize for LevelData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LevelData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_json(&value).map_err(D::Error::custom)
    }
}

/// Parse a versioned authored level and return an owned, validated copy.
pub fn parse_level(text: &str) -> Result<LevelData, LevelError> {
    let value: Value = serde_json::from_str(text).map_err(|error| LevelError::Json(error.to_string()))?;
    LevelData::from_json(&value)
}

/// Serialize a level as pretty JSON with a trailing newline; the layout is fixed so saved levels stay byte-stable.
///
/// The level is written as-is; call [`LevelData::validate`] first when it was assembled from untrusted edits.
pub fn serialize_level(level: &LevelData) -> String {
    let mut text = serde_json::to_string_pretty(&level.to_json()).expect("JSON values always serialize");
    text.push('\n');
    text
}

/// Owns authored level state, the editor selection, and up to [`LEVEL_HISTORY_LIMIT`] undo transactions (used by Joy
/// Editor).
///
/// Every stored snapshot is validated and canonical (as produced by [`LevelData::from_json`]). Edits run on isolated
/// drafts and commit atomically; simulation and rendering must work from [`LevelDocument::snapshot`] copies.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelDocument {
    // Current authored state and its serialized form, used to detect edits that change nothing.
    level: LevelData,
    level_text: String,

    // Undo snapshots (oldest first) and redo snapshots (most recently undone last).
    past: VecDeque<LevelData>,
    future: Vec<LevelData>,

    // Selection is UI state, not an authored edit.
    selected_id: Option<String>,
}

impl LevelDocument {
    /// Validate and copy a level into a document with empty history and no selection.
    pub fn new(level: &LevelData) -> Result<Self, LevelError> {
        let level = canonical_level(level)?;
        let level_text = level_fingerprint(&level);
        Ok(Self {
            level,
            level_text,
            past: VecDeque::new(),
            future: Vec::new(),
            selected_id: None,
        })
    }

    /// An owned copy suitable for rendering, simulation, or editing.
    pub fn snapshot(&self) -> LevelData {
        self.level.clone()
    }

    /// Borrow the current authored state without copying it.
    pub fn level(&self) -> &LevelData {
        &self.level
    }

    /// Whether an earlier authored snapshot is available.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// Whether an undone authored snapshot is available.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// The selected entity id, if it names an entity in the current level.
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    /// Select an entity by id, or clear the selection. Ids not in the current level clear it. Not an authored edit.
    pub fn select(&mut self, id: Option<&str>) {
        self.selected_id = id.filter(|id| self.level.entities.iter().any(|entity| entity.id == *id)).map(str::to_owned);
    }

    /// Apply one atomic edit to an isolated draft and return whether authored data changed.
    ///
    /// The draft is validated like a parsed level; an invalid result fails and keeps the prior state and history. A
    /// change pushes the previous state onto the undo history (dropping the oldest beyond the limit), clears redo, and
    /// revalidates the selection. Edits that serialize identically change nothing.
    pub fn transact(&mut self, edit: impl FnOnce(&mut LevelData)) -> Result<bool, LevelError> {
        self.try_transact(|draft| {
            edit(draft);
            Ok::<(), LevelError>(())
        })
    }

    /// [`LevelDocument::transact`] for edits that can fail: an edit error is returned unchanged and nothing changes.
    pub fn try_transact<E: From<LevelError>>(
        &mut self,
        edit: impl FnOnce(&mut LevelData) -> Result<(), E>,
    ) -> Result<bool, E> {
        let mut draft = self.snapshot();
        edit(&mut draft)?;
        let next = canonical_level(&draft)?;
        let next_text = level_fingerprint(&next);
        if next_text == self.level_text {
            return Ok(false);
        }

        let previous = std::mem::replace(&mut self.level, next);
        self.level_text = next_text;
        self.past.push_back(previous);
        if self.past.len() > LEVEL_HISTORY_LIMIT {
            self.past.pop_front();
        }
        self.future.clear();
        self.revalidate_selection();
        Ok(true)
    }

    /// Restore one authored transaction; an invalidated selection is cleared. Returns whether anything was undone.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.past.pop_back() else {
            return false;
        };
        let current = std::mem::replace(&mut self.level, previous);
        self.level_text = level_fingerprint(&self.level);
        self.future.push(current);
        self.revalidate_selection();
        true
    }

    /// Reapply one undone transaction; an invalidated selection is cleared. Returns whether anything was redone.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.level, next);
        self.level_text = level_fingerprint(&self.level);
        self.past.push_back(current);
        self.revalidate_selection();
        true
    }

    /// Clear the selection when its entity no longer exists.
    fn revalidate_selection(&mut self) {
        let selected = self.selected_id.take();
        self.select(selected.as_deref());
    }
}

/// Validate a level and return its canonical copy.
fn canonical_level(level: &LevelData) -> Result<LevelData, LevelError> {
    LevelData::from_json(&level.to_json())
}

/// Order-sensitive serialized form used to detect no-op edits; a change in key or entity order counts as an edit.
fn level_fingerprint(level: &LevelData) -> String {
    level.to_json().to_string()
}
