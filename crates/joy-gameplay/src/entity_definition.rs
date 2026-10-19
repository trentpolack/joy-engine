// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Authored entity definitions: the serialized record shared by levels, objects, and runtime spawning.
//!
//! The id must be a nonblank string, the name
//! defaults to the id, the type defaults to `entity`, transforms are three finite numbers per vector, components are
//! JSON objects preserved verbatim, and a `template` link must be a `.joyobject` asset path whose `overrides` may only
//! touch `type`, `tags`, and `components`. Unknown top-level keys are dropped.
//!
//! Serialization writes keys in the established document order (`template`, `overrides`, `id`, `name`, `type`, `tags`,
//! `transform`, `components`) and writes transform numbers the way JavaScript does (integers without a decimal point),
//! so documents round trip byte for byte. Transforms are stored as `f32`: authored values that need more than single
//! precision are rounded to the nearest `f32` and written back in its shortest decimal form.

use indexmap::IndexMap;
use joy_core::math::Vec3;
use joy_core::{
    JsonRecord,
    Transform,
};
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

use super::asset_path::asset_path;
use super::component_vocabulary::entity_type;
use super::error::{
    AssetPathError,
    DefinitionError,
};

/// Extension required for object template links.
pub(crate) const OBJECT_EXTENSION: &str = ".joyobject";

/// Keys an object instance may override.
const OVERRIDE_KEYS: [&str; 3] = ["type", "tags", "components"];

/// Largest magnitude written as a JSON integer (JavaScript's safe integer range).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Serialized transform field names (TypeScript `TRANSFORM_PROPERTY`).
mod transform_property {
    pub const POSITION: &str = "position";
    pub const ROTATION: &str = "rotation";
    pub const SCALE: &str = "scale";
}

// ---------------------------------------------------------------------------------------------------------------------
// Transform
// ---------------------------------------------------------------------------------------------------------------------

/// Authored placement: world position in meters, XYZ Euler rotation in radians (applied X, then Y, then Z), and
/// per-axis scale applied before rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityTransform {
    /// World position in meters.
    pub position: Vec3,
    /// XYZ Euler radians applied X, then Y, then Z.
    pub rotation: Vec3,
    /// Per-axis scale.
    pub scale: Vec3,
}

impl EntityTransform {
    /// No translation, no rotation, and unit scale.
    pub const IDENTITY: Self = Self { position: Vec3::ZERO, rotation: Vec3::ZERO, scale: Vec3::ONE };

    /// Whether every component is finite.
    pub fn is_finite(&self) -> bool {
        self.position.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }

    /// Read an authored transform record, applying defaults for omitted vectors.
    pub fn from_json(value: &Value) -> Result<Self, DefinitionError> {
        let Value::Object(record) = value else {
            return Err(DefinitionError::InvalidRecords);
        };
        let read = |name: &'static str, fallback: Vec3| match record.get(name) {
            None => Ok(fallback),
            Some(entry) => read_vector(entry).ok_or(DefinitionError::InvalidVector(name)),
        };
        Ok(Self {
            position: read(transform_property::POSITION, Vec3::ZERO)?,
            rotation: read(transform_property::ROTATION, Vec3::ZERO)?,
            scale: read(transform_property::SCALE, Vec3::ONE)?,
        })
    }

    /// Serialize as `{position, rotation, scale}` with JavaScript-style numbers.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        record.insert(transform_property::POSITION.to_owned(), vector_to_json(self.position));
        record.insert(transform_property::ROTATION.to_owned(), vector_to_json(self.rotation));
        record.insert(transform_property::SCALE.to_owned(), vector_to_json(self.scale));
        Value::Object(record)
    }
}

impl Default for EntityTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl From<Transform> for EntityTransform {
    fn from(transform: Transform) -> Self {
        Self { position: transform.position, rotation: transform.rotation, scale: transform.scale }
    }
}

impl From<EntityTransform> for Transform {
    fn from(transform: EntityTransform) -> Self {
        Transform::new(transform.position, transform.rotation, transform.scale)
    }
}

impl Serialize for EntityTransform {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EntityTransform {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_json(&value).map_err(D::Error::custom)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Definition
// ---------------------------------------------------------------------------------------------------------------------

/// Validated, owned entity definition.
///
/// Fields are public so tools can edit definitions; [`EntityDefinition::validate`] re-checks a programmatically built
/// definition, and runtime spawning always validates before creating anything.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityDefinition {
    /// Stable, nonblank identifier, unique within a level.
    pub id: String,
    /// Display name; defaults to the id.
    pub name: String,
    /// Entity type used to select a registered factory (serialized as `type`); defaults to `entity`.
    pub kind: String,
    /// Free-form tags.
    pub tags: Vec<String>,
    /// Authored placement.
    pub transform: EntityTransform,
    /// Authored component records by type, in authored order. The engine interprets only its own component types and
    /// preserves every other record unchanged.
    pub components: IndexMap<String, JsonRecord>,
    /// Linked `.joyobject` asset path, if this is an object instance.
    pub template: Option<String>,
    /// Local overrides of the linked object's `type`, `tags`, and `components`; present exactly when `template` is.
    pub overrides: Option<JsonRecord>,
}

impl EntityDefinition {
    /// A plain `entity` with the given id (also used as its name), identity transform, and no components.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            kind: entity_type::ENTITY.to_owned(),
            tags: Vec::new(),
            transform: EntityTransform::IDENTITY,
            components: IndexMap::new(),
            template: None,
            overrides: None,
        }
    }

    /// Validate an authored JSON definition and return an owned copy with defaults applied.
    ///
    /// Keys that are present must have the right type (`null` is not "omitted"). Unknown top-level keys are dropped.
    pub fn from_json(value: &Value) -> Result<Self, DefinitionError> {
        let Value::Object(record) = value else {
            return Err(DefinitionError::NotAnObject);
        };

        let id = match record.get("id") {
            Some(Value::String(id)) if !id.trim().is_empty() => id.clone(),
            _ => return Err(DefinitionError::InvalidId),
        };
        let name = match record.get("name") {
            None => id.clone(),
            Some(Value::String(name)) => name.clone(),
            Some(_) => return Err(DefinitionError::InvalidNameOrType),
        };
        let kind = match record.get("type") {
            None => entity_type::ENTITY.to_owned(),
            Some(Value::String(kind)) if !kind.trim().is_empty() => kind.clone(),
            Some(_) => return Err(DefinitionError::InvalidNameOrType),
        };
        let tags = match record.get("tags") {
            None => Vec::new(),
            Some(Value::Array(tags)) => tags
                .iter()
                .map(|tag| tag.as_str().map(str::to_owned).ok_or(DefinitionError::InvalidTags))
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err(DefinitionError::InvalidTags),
        };

        let transform_value = record.get("transform");
        let components_value = record.get("components");
        let transform_is_record = transform_value.is_none_or(Value::is_object);
        let components_is_record = components_value.is_none_or(Value::is_object);
        if !transform_is_record || !components_is_record {
            return Err(DefinitionError::InvalidRecords);
        }

        let mut components = IndexMap::new();
        if let Some(Value::Object(authored)) = components_value {
            for (key, value) in authored {
                let Value::Object(component) = value else {
                    return Err(DefinitionError::InvalidComponent(key.clone()));
                };
                components.insert(key.clone(), component.clone());
            }
        }

        let template = match record.get("template") {
            None => None,
            Some(Value::String(path)) => Some(asset_path(path, &[OBJECT_EXTENSION])?.to_owned()),
            Some(_) => return Err(AssetPathError::OutsideAssets.into()),
        };
        let overrides = match (record.get("overrides"), &template) {
            (None, None) => None,
            (None, Some(_)) => Some(JsonRecord::new()),
            (Some(Value::Object(overrides)), Some(_)) => Some(overrides.clone()),
            (Some(_), _) => return Err(DefinitionError::InvalidOverrides),
        };
        if let Some(overrides) = &overrides {
            validate_override_keys(overrides)?;
        }

        // The transform is read last, matching the order in which the TypeScript copy reports vector errors.
        let transform = match transform_value {
            None => EntityTransform::IDENTITY,
            Some(value) => EntityTransform::from_json(value)?,
        };

        Ok(Self { id, name, kind, tags, transform, components, template, overrides })
    }

    /// Serialize in the TypeScript key order. Overrides are written (as `{}` if absent) whenever a template is linked.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        if let Some(template) = &self.template {
            record.insert("template".to_owned(), Value::String(template.clone()));
            record.insert("overrides".to_owned(), Value::Object(self.overrides.clone().unwrap_or_default()));
        }
        record.insert("id".to_owned(), Value::String(self.id.clone()));
        record.insert("name".to_owned(), Value::String(self.name.clone()));
        record.insert("type".to_owned(), Value::String(self.kind.clone()));
        record.insert("tags".to_owned(), Value::Array(self.tags.iter().cloned().map(Value::String).collect()));
        record.insert("transform".to_owned(), self.transform.to_json());
        let components = self.components.iter().map(|(key, value)| (key.clone(), Value::Object(value.clone())));
        record.insert("components".to_owned(), Value::Object(components.collect()));
        Value::Object(record)
    }

    /// Re-check a definition that was built or edited in code, with the same rules as [`EntityDefinition::from_json`].
    pub fn validate(&self) -> Result<(), DefinitionError> {
        if self.id.trim().is_empty() {
            return Err(DefinitionError::InvalidId);
        }
        if self.kind.trim().is_empty() {
            return Err(DefinitionError::InvalidNameOrType);
        }
        if let Some(template) = &self.template {
            asset_path(template, &[OBJECT_EXTENSION])?;
        }
        match (&self.overrides, &self.template) {
            (Some(_), None) => return Err(DefinitionError::InvalidOverrides),
            (Some(overrides), Some(_)) => validate_override_keys(overrides)?,
            _ => {}
        }
        let vectors = [
            (transform_property::POSITION, self.transform.position),
            (transform_property::ROTATION, self.transform.rotation),
            (transform_property::SCALE, self.transform.scale),
        ];
        for (name, vector) in vectors {
            if !vector.is_finite() {
                return Err(DefinitionError::InvalidVector(name));
            }
        }
        Ok(())
    }
}

impl Serialize for EntityDefinition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EntityDefinition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_json(&value).map_err(D::Error::custom)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Reject override keys outside `type`, `tags`, and `components`.
fn validate_override_keys(overrides: &JsonRecord) -> Result<(), DefinitionError> {
    if overrides.keys().any(|key| !OVERRIDE_KEYS.contains(&key.as_str())) {
        return Err(DefinitionError::UnsupportedOverride);
    }
    Ok(())
}

/// Read exactly three finite JSON numbers that remain finite as `f32`, as component records store vectors and colors.
pub fn read_vector(value: &Value) -> Option<Vec3> {
    let Value::Array(items) = value else {
        return None;
    };
    if items.len() != 3 {
        return None;
    }
    let mut components = [0.0_f32; 3];
    for (slot, item) in components.iter_mut().zip(items) {
        let number = item.as_f64()? as f32;
        if !number.is_finite() {
            return None;
        }
        *slot = number;
    }
    Some(Vec3::from_array(components))
}

/// Write a vector as three JSON numbers in the [`number_to_json`] spelling.
pub fn vector_to_json(vector: Vec3) -> Value {
    Value::Array(vector.to_array().into_iter().map(number_to_json).collect())
}

/// Write an `f32` as an authored document stores it.
///
/// Integral values are written without a decimal point (and `-0` as `0`); other values use the shortest decimal that
/// round trips the `f32`, so `0.1f32` is written as `0.1` rather than its exact binary expansion. Non-finite values,
/// which JSON cannot represent, are written as `null`.
pub fn number_to_json(value: f32) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    let widened = f64::from(value);
    if widened.fract() == 0.0 && widened.abs() <= MAX_SAFE_INTEGER {
        return Value::from(widened as i64);
    }
    // Display prints the shortest decimal that parses back to the same f32; re-reading it as f64 keeps that spelling.
    let shortest = value.to_string().parse::<f64>().unwrap_or(widened);
    Value::from(shortest)
}

#[cfg(test)]
#[path = "entity_definition.test.rs"]
mod tests;
