// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Versioned `.joylevel` documents.
//!
//! Parsing validates the version, a nonblank name, every entity definition, unique entity ids, and the optional
//! post-processing profile, exactly like the TypeScript `parseLevel`. Serialization writes pretty JSON with two-space
//! indentation, the TypeScript key order, and a trailing newline, so documents written by either engine round trip
//! unchanged. Component data is never evaluated.

use joy_foundation::PostProcessingProfile;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::definition::EntityDefinition;
use super::error::LevelError;

/// The only supported level format version.
pub const LEVEL_VERSION: u32 = 1;

/// Owned, validated level data.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelData {
    /// Format version; always [`LEVEL_VERSION`].
    pub version: u32,
    /// Nonblank display name.
    pub name: String,
    /// Entity definitions in authored order; ids are unique.
    pub entities: Vec<EntityDefinition>,
    /// Optional display profile, validated by `PostProcessingProfile::validate`.
    pub post_processing: Option<PostProcessingProfile>,
}

impl LevelData {
    /// An empty level with a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self { version: LEVEL_VERSION, name: name.into(), entities: Vec::new(), post_processing: None }
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

        // A present key is validated even when null, matching `Object.hasOwn` in the TypeScript engine.
        let post_processing = match record.get("postProcessing") {
            None => None,
            Some(profile) => Some(PostProcessingProfile::validate(profile)?),
        };
        Ok(Self { version: LEVEL_VERSION, name, entities, post_processing })
    }

    /// Serialize in the TypeScript key order: `version`, `name`, `entities`, then `postProcessing` when present.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        record.insert("version".to_owned(), Value::from(self.version));
        record.insert("name".to_owned(), Value::String(self.name.clone()));
        record
            .insert("entities".to_owned(), Value::Array(self.entities.iter().map(EntityDefinition::to_json).collect()));
        if let Some(profile) = &self.post_processing {
            let profile = serde_json::to_value(profile).expect("post-processing profiles always serialize to JSON");
            record.insert("postProcessing".to_owned(), profile);
        }
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

/// Serialize a level as pretty JSON with a trailing newline, byte-compatible with the TypeScript `serializeLevel`.
///
/// The level is written as-is; call [`LevelData::validate`] first when it was assembled from untrusted edits.
pub fn serialize_level(level: &LevelData) -> String {
    let mut text = serde_json::to_string_pretty(&level.to_json()).expect("JSON values always serialize");
    text.push('\n');
    text
}
