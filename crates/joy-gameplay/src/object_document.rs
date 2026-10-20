// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Reusable single-object documents (`.joyobject`) and linked object instances.
//!
//! An instance keeps its own identity (`id`, `name`) and placement (`transform`) and links a source object through
//! `template`. Its `overrides` record replaces the source's `type`, `tags`, and `components` key by key: nested objects
//! merge recursively, arrays and scalars replace as a unit, and a `null` component override removes that component.
//! Linked definitions cannot nest.

use std::collections::HashMap;
use std::fmt::Display;

use serde_json::{
    Map,
    Value,
};

use super::asset_path::asset_path;
use super::entity_definition::{
    EntityDefinition,
    OBJECT_EXTENSION,
};
use super::error::ObjectError;
use super::level_document::LevelData;

/// The only supported object format version.
pub const OBJECT_VERSION: u32 = 1;

/// Owned, validated reusable object.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectDocument {
    /// Format version; always [`OBJECT_VERSION`].
    pub version: u32,
    /// Nonblank display name, used as the name of new instances.
    pub name: String,
    /// The reusable entity; it never links another template.
    pub entity: EntityDefinition,
}

impl ObjectDocument {
    /// Validate a parsed JSON object document.
    pub fn from_json(value: &Value) -> Result<Self, ObjectError> {
        let Value::Object(record) = value else {
            return Err(ObjectError::InvalidObject);
        };
        let version_ok = record.get("version").and_then(Value::as_f64) == Some(f64::from(OBJECT_VERSION));
        let name = match record.get("name") {
            Some(Value::String(name)) if version_ok && !name.trim().is_empty() => name.clone(),
            _ => return Err(ObjectError::InvalidObject),
        };
        let entity = EntityDefinition::from_json(record.get("entity").unwrap_or(&Value::Null))?;
        if entity.template.is_some() || entity.overrides.is_some() {
            return Err(ObjectError::NestedTemplate);
        }
        Ok(Self {
            version: OBJECT_VERSION,
            name,
            entity,
        })
    }

    /// Serialize as `{version, name, entity}`.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        record.insert("version".to_owned(), Value::from(self.version));
        record.insert("name".to_owned(), Value::String(self.name.clone()));
        record.insert("entity".to_owned(), self.entity.to_json());
        Value::Object(record)
    }
}

/// Parse a reusable single-object document.
pub fn parse_object(text: &str) -> Result<ObjectDocument, ObjectError> {
    let value: Value = serde_json::from_str(text).map_err(|error| ObjectError::Json(error.to_string()))?;
    ObjectDocument::from_json(&value)
}

/// Validate and serialize an object as pretty JSON with a trailing newline.
pub fn serialize_object(object: &ObjectDocument) -> Result<String, ObjectError> {
    let validated = ObjectDocument::from_json(&object.to_json())?;
    let mut text = serde_json::to_string_pretty(&validated.to_json()).expect("JSON values always serialize");
    text.push('\n');
    Ok(text)
}

/// Create an independently placed instance linked to a reusable object.
///
/// The instance takes `id`, the object's name, the object's transform, a `template` link to `path` (which must be a
/// safe `.joyobject` asset path), and empty overrides.
pub fn create_object_instance(object: &ObjectDocument, path: &str, id: &str) -> Result<EntityDefinition, ObjectError> {
    let source = ObjectDocument::from_json(&object.to_json())?;
    let template = asset_path(path, &[OBJECT_EXTENSION])?;
    let mut record = Map::new();
    record.insert("id".to_owned(), Value::String(id.to_owned()));
    record.insert("name".to_owned(), Value::String(source.name));
    record.insert("template".to_owned(), Value::String(template.to_owned()));
    record.insert("transform".to_owned(), source.entity.transform.to_json());
    record.insert("overrides".to_owned(), Value::Object(Map::new()));
    Ok(EntityDefinition::from_json(&Value::Object(record))?)
}

/// Resolve an instance's overrides over the current source object.
///
/// Identity and placement stay local; the result has no template link. Null component overrides remove that
/// component; arrays replace as a unit. Neither input is modified.
pub fn resolve_object_instance(
    entity: &EntityDefinition,
    object: &ObjectDocument,
) -> Result<EntityDefinition, ObjectError> {
    let instance = EntityDefinition::from_json(&entity.to_json())?;
    let definition = ObjectDocument::from_json(&object.to_json())?.entity;

    let mut base = Map::new();
    base.insert("type".to_owned(), Value::String(definition.kind));
    base.insert("tags".to_owned(), Value::Array(definition.tags.into_iter().map(Value::String).collect()));
    let components = definition.components.into_iter().map(|(key, value)| (key, Value::Object(value)));
    base.insert("components".to_owned(), Value::Object(components.collect()));
    let mut values = merge_records(&base, instance.overrides.as_ref().unwrap_or(&Map::new()));

    // Null component overrides delete the inherited component.
    if let Some(Value::Object(components)) = values.get_mut("components") {
        components.retain(|_, value| !value.is_null());
    }

    let mut record = Map::new();
    record.insert("id".to_owned(), Value::String(instance.id));
    record.insert("name".to_owned(), Value::String(instance.name));
    record.insert("transform".to_owned(), instance.transform.to_json());
    for key in ["type", "tags", "components"] {
        if let Some(value) = values.remove(key) {
            record.insert(key.to_owned(), value);
        }
    }
    Ok(EntityDefinition::from_json(&Value::Object(record))?)
}

/// Resolve every linked object in a level, reading each source path once.
///
/// `read` receives validated `.joyobject` asset paths and returns the document text; its errors are reported as
/// [`ObjectError::Read`]. The input level is not modified; the returned copy has no template links.
pub fn resolve_level_objects<E: Display>(
    level: &LevelData,
    mut read: impl FnMut(&str) -> Result<String, E>,
) -> Result<LevelData, ObjectError> {
    let mut copy = LevelData::from_json(&level.to_json())?;
    let mut sources: HashMap<String, ObjectDocument> = HashMap::new();
    for entity in &mut copy.entities {
        let Some(template) = entity.template.clone() else {
            continue;
        };
        let path = asset_path(&template, &[OBJECT_EXTENSION])?.to_owned();
        if !sources.contains_key(&path) {
            let text = read(&path).map_err(|error| ObjectError::Read {
                path: path.clone(),
                message: error.to_string(),
            })?;
            sources.insert(path.clone(), parse_object(&text)?);
        }
        if let Some(source) = sources.get(&path) {
            *entity = resolve_object_instance(entity, source)?;
        }
    }
    Ok(copy)
}

/// Merge override records over a base: objects merge recursively, everything else replaces.
fn merge_records(base: &Map<String, Value>, overrides: &Map<String, Value>) -> Map<String, Value> {
    let mut result = base.clone();
    for (key, value) in overrides {
        let next = match (value, base.get(key)) {
            (Value::Object(override_record), Some(Value::Object(original))) => {
                Value::Object(merge_records(original, override_record))
            }
            _ => value.clone(),
        };
        result.insert(key.clone(), next);
    }
    result
}
