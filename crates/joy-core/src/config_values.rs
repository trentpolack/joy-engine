// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Typed tuning and configuration values: numeric field contracts, validated value records, version-1 override
//! documents, and the JSON Schema editors use.
//!
//! Games declare a list of [`ConfigField`]s (their CONFIG panel), merge saved overrides into a complete record with
//! [`create_config_values`] or [`create_config_document_values`], and generate the persistence schema with
//! [`create_config_schema`]. Values are JavaScript numbers (`f64`); records keep field order. Validation order and
//! error messages preserve the authored format's established validation contract.

use indexmap::IndexMap;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    Map,
    Value,
};

use crate::format::{
    format_number,
    json_number,
};

/// JSON Schema dialect written by [`create_config_schema`].
const JSON_SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// The only supported override document version.
pub const CONFIG_DOCUMENT_VERSION: u32 = 1;

/// Keys an override document may contain.
const DOCUMENT_KEYS: [&str; 3] = ["$schema", "version", "values"];

/// A field step of exactly one marks an integer field.
const INTEGER_STEP: f64 = 1.0;

/// When a changed value takes effect, serialized with the TypeScript `TUNING_APPLY_MODE` spellings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigApplyMode {
    /// Applies on the next frame.
    Live,
    /// Applies to entities spawned after the change.
    NextSpawn,
    /// Applies after the game restarts.
    Restart,
}

impl ConfigApplyMode {
    /// Serialized name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::NextSpawn => "next-spawn",
            Self::Restart => "restart",
        }
    }
}

/// One numeric tuning field. Bounds are inclusive; a `step` of exactly 1 makes the field an integer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    /// Stable, nonempty, unique record key.
    pub key: String,
    /// Display label.
    pub label: String,
    /// Display group.
    pub group: String,
    /// One sentence of help, also written into the schema.
    pub description: String,
    /// Shipped value; must satisfy the field's own bounds.
    pub default_value: f64,
    /// Inclusive minimum.
    pub min: f64,
    /// Inclusive maximum; at least `min`.
    pub max: f64,
    /// Positive control increment; exactly 1 means integer values.
    pub step: f64,
    /// Display units, for example `m` or `s`.
    pub units: String,
    /// When a changed value takes effect.
    pub apply_mode: ConfigApplyMode,
}

/// A complete, independently owned value record keyed by field key, in field order.
pub type ConfigValues = IndexMap<String, f64>;

/// Invalid field metadata, overrides, or documents. Messages match the TypeScript engine.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ConfigError {
    /// A field had an empty key.
    #[error("Every config field requires a non-empty key.")]
    EmptyKey,
    /// Two fields shared a key.
    #[error("Duplicate config key \"{0}\".")]
    DuplicateKey(String),
    /// A field's default, bounds, or step was not finite.
    #[error("Config field \"{0}\" numeric metadata must be finite.")]
    NonFiniteMetadata(String),
    /// A field's minimum exceeded its maximum, or its step was not positive.
    #[error("Config field \"{0}\" requires min <= max and a positive step.")]
    InvalidBounds(String),
    /// A value or override named no field.
    #[error("Unknown config key \"{0}\".")]
    UnknownKey(String),
    /// A value was not a finite number.
    #[error("Config value \"{0}\" must be finite.")]
    NonFiniteValue(String),
    /// A value was below its field's minimum.
    #[error("Config value \"{key}\" is below its minimum of {}.", format_number(*.minimum))]
    BelowMinimum {
        /// Field key.
        key: String,
        /// The field's minimum.
        minimum: f64,
    },
    /// A value exceeded its field's maximum.
    #[error("Config value \"{key}\" exceeds its maximum of {}.", format_number(*.maximum))]
    AboveMaximum {
        /// Field key.
        key: String,
        /// The field's maximum.
        maximum: f64,
    },
    /// An integer field received a fractional value.
    #[error("Config value \"{0}\" must be an integer.")]
    NotAnInteger(String),
    /// The override document was not a JSON object.
    #[error("Config document must be an object.")]
    DocumentNotAnObject,
    /// The override document had a key other than `$schema`, `version`, and `values`.
    #[error("Unknown config document key \"{0}\".")]
    UnknownDocumentKey(String),
    /// The override document's version was not 1.
    #[error("Config document version must be 1.")]
    UnsupportedVersion,
    /// The override document's `$schema` was present but not a string.
    #[error("Config document $schema must be a string.")]
    InvalidSchemaReference,
    /// The override document's `values` was not a JSON object.
    #[error("Config document values must be an object.")]
    ValuesNotAnObject,
}

/// Validate field metadata and merge partial overrides into a new complete record.
///
/// Overrides naming unknown keys fail first (in override order); then each field, in field order, takes its override
/// or default and is checked against its bounds. The result is owned by the caller and keeps field order.
pub fn create_config_values(
    fields: &[ConfigField],
    overrides: &IndexMap<String, f64>,
) -> Result<ConfigValues, ConfigError> {
    merge_values(fields, overrides.iter().map(|(key, value)| (key.as_str(), *value)))
}

/// Validate a serialized version-1 override document (untrusted parsed JSON) and return complete values.
///
/// The document must be an object with only `$schema` (a string, optional), `version` (exactly 1), and `values` (an
/// object of overrides). Override values that are not JSON numbers fail as non-finite, like the TypeScript check.
pub fn create_config_document_values(fields: &[ConfigField], document: &Value) -> Result<ConfigValues, ConfigError> {
    let Value::Object(record) = document else {
        return Err(ConfigError::DocumentNotAnObject);
    };
    if let Some(key) = record.keys().find(|key| !DOCUMENT_KEYS.contains(&key.as_str())) {
        return Err(ConfigError::UnknownDocumentKey(key.clone()));
    }
    if record.get("version").and_then(Value::as_f64) != Some(f64::from(CONFIG_DOCUMENT_VERSION)) {
        return Err(ConfigError::UnsupportedVersion);
    }
    if record.get("$schema").is_some_and(|schema| !schema.is_string()) {
        return Err(ConfigError::InvalidSchemaReference);
    }
    let Some(Value::Object(values)) = record.get("values") else {
        return Err(ConfigError::ValuesNotAnObject);
    };

    // Non-numeric overrides become NaN so they fail the finite check at their field, in field order.
    merge_values(fields, values.iter().map(|(key, value)| (key.as_str(), value.as_f64().unwrap_or(f64::NAN))))
}

/// Produce the strict JSON Schema (draft 2020-12) used by editors and persistence boundaries.
///
/// Values stay optional because override files only store authored differences. Key order and number spelling match
/// the TypeScript `createConfigSchema`, so `serde_json::to_string_pretty` writes the same text as
/// `JSON.stringify(schema, null, 2)`.
pub fn create_config_schema(fields: &[ConfigField]) -> Result<Value, ConfigError> {
    validate_fields(fields)?;

    let mut properties = Map::new();
    for field in fields {
        let kind = if field.step == INTEGER_STEP { "integer" } else { "number" };
        let mut property = Map::new();
        property.insert("type".to_owned(), Value::from(kind));
        property.insert("minimum".to_owned(), json_number(field.min));
        property.insert("maximum".to_owned(), json_number(field.max));
        property.insert("description".to_owned(), Value::from(field.description.clone()));
        properties.insert(field.key.clone(), Value::Object(property));
    }

    let mut values = Map::new();
    values.insert("type".to_owned(), Value::from("object"));
    values.insert("additionalProperties".to_owned(), Value::Bool(false));
    values.insert("properties".to_owned(), Value::Object(properties));

    let mut schema_reference = Map::new();
    schema_reference.insert("type".to_owned(), Value::from("string"));
    let mut version = Map::new();
    version.insert("const".to_owned(), Value::from(CONFIG_DOCUMENT_VERSION));

    let mut document_properties = Map::new();
    document_properties.insert("$schema".to_owned(), Value::Object(schema_reference));
    document_properties.insert("version".to_owned(), Value::Object(version));
    document_properties.insert("values".to_owned(), Value::Object(values));

    let mut schema = Map::new();
    schema.insert("$schema".to_owned(), Value::from(JSON_SCHEMA_DIALECT));
    schema.insert("type".to_owned(), Value::from("object"));
    schema.insert("additionalProperties".to_owned(), Value::Bool(false));
    schema.insert("required".to_owned(), Value::from(vec!["version", "values"]));
    schema.insert("properties".to_owned(), Value::Object(document_properties));
    Ok(Value::Object(schema))
}

// ---------------------------------------------------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Shared merge: unknown override keys first, then every field in order.
fn merge_values<'a>(
    fields: &[ConfigField],
    overrides: impl Iterator<Item = (&'a str, f64)>,
) -> Result<ConfigValues, ConfigError> {
    validate_fields(fields)?;
    let mut supplied: IndexMap<&str, f64> = IndexMap::new();
    for (key, value) in overrides {
        if !fields.iter().any(|field| field.key == key) {
            return Err(ConfigError::UnknownKey(key.to_owned()));
        }
        supplied.insert(key, value);
    }

    let mut values = ConfigValues::with_capacity(fields.len());
    for field in fields {
        let value = supplied.get(field.key.as_str()).copied().unwrap_or(field.default_value);
        validate_value(field, value)?;
        values.insert(field.key.clone(), value);
    }
    Ok(values)
}

/// Validate field metadata and uniqueness in field order.
fn validate_fields(fields: &[ConfigField]) -> Result<(), ConfigError> {
    for (index, field) in fields.iter().enumerate() {
        if field.key.is_empty() {
            return Err(ConfigError::EmptyKey);
        }
        if fields[..index].iter().any(|earlier| earlier.key == field.key) {
            return Err(ConfigError::DuplicateKey(field.key.clone()));
        }
        let metadata = [field.default_value, field.min, field.max, field.step];
        if !metadata.iter().all(|value| value.is_finite()) {
            return Err(ConfigError::NonFiniteMetadata(field.key.clone()));
        }
        if field.min > field.max || field.step <= 0.0 {
            return Err(ConfigError::InvalidBounds(field.key.clone()));
        }
        validate_value(field, field.default_value)?;
    }
    Ok(())
}

/// Check one value against its field: finite, within bounds, and whole for integer fields.
fn validate_value(field: &ConfigField, value: f64) -> Result<(), ConfigError> {
    if !value.is_finite() {
        return Err(ConfigError::NonFiniteValue(field.key.clone()));
    }
    if value < field.min {
        return Err(ConfigError::BelowMinimum { key: field.key.clone(), minimum: field.min });
    }
    if value > field.max {
        return Err(ConfigError::AboveMaximum { key: field.key.clone(), maximum: field.max });
    }
    if field.step == INTEGER_STEP && value.fract() != 0.0 {
        return Err(ConfigError::NotAnInteger(field.key.clone()));
    }
    Ok(())
}
