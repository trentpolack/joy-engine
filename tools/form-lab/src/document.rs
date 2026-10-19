// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM LAB documents (`.formlab`): script source with parameter overrides, a document material palette, and named
//! parameter variants.
//!
//! Documents are validated once when read, so the editor, the compiler, and saves only ever see reopenable data. A raw
//! `.form` file is script source alone; [`FormDocument::from_script`] wraps it without inventing overrides. Serialized
//! documents are compact JSON with stable key order (`version`, `name`, `source`, `overrides`, `materials`,
//! `variants`), shared by standalone and hosted FORM LAB, Joy Editor, and the engine's scene asset loader.

use joy_engine::form::{
    DOCUMENT_FILE_BYTES,
    read_materials,
    read_parameter_value,
};
use joy_engine::serde_json::{
    self,
    Map,
    Value as Json,
};

use crate::export::integral_numbers_as_integers;

/// The only document version.
pub const DOCUMENT_VERSION: u64 = 1;

/// Largest script, in characters (the FORM source budget).
pub const MAXIMUM_SOURCE_LENGTH: usize = 200_000;

/// Longest document name, in characters.
pub const MAXIMUM_NAME_LENGTH: usize = 120;

/// Most variants one document keeps.
pub const MAXIMUM_VARIANTS: usize = 16;

/// Longest variant name, in characters.
pub const MAXIMUM_VARIANT_NAME_LENGTH: usize = 64;

/// Parameter overrides keyed by parameter name, in insertion order: finite scalars, XYZ vectors, RGBA colors, and
/// mesh or material resources as FORM serializes them.
pub type Overrides = Map<String, Json>;

/// Why a document was rejected. Messages match the TypeScript tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    /// The text exceeds [`DOCUMENT_FILE_BYTES`].
    TooLarge,
    /// The text is not JSON.
    Json(String),
    /// The JSON is not a version 1 document with a name and source within their limits.
    Invalid,
    /// The overrides are not an object of valid parameter values.
    Overrides(String),
    /// The variants are not a list of at most 16 uniquely named override sets.
    Variants(String),
    /// The material palette failed FORM's validation.
    Materials(String),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("FORM LAB documents must be 64 MB or smaller."),
            Self::Json(message) => write!(formatter, "This file is not valid JSON: {message}"),
            Self::Invalid => formatter.write_str("This is not a valid FORM LAB version 1 document."),
            Self::Overrides(message) | Self::Variants(message) | Self::Materials(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for DocumentError {}

/// One named set of explicit parameter overrides. Script defaults stay live for parameters it leaves out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParameterVariant {
    /// Trimmed, unique (ignoring case) name of 1–64 characters.
    pub name: String,
    /// The take's overrides.
    pub overrides: Overrides,
}

/// A validated FORM LAB document. The document owns every value; editing a copy never changes another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormDocument {
    /// Display name of at most 120 characters.
    pub name: String,
    /// FORM script source of at most 200,000 characters.
    pub source: String,
    /// Parameter overrides applied over the script's defaults.
    pub overrides: Overrides,
    /// Validated material palette (Default first), when the document carries one.
    pub materials: Option<Vec<Json>>,
    /// Named parameter variants, when the document carries them.
    pub variants: Option<Vec<ParameterVariant>>,
}

impl FormDocument {
    /// A document for raw script source, as FORM LAB opens `.form` files: no overrides, palette, or variants.
    pub fn from_script(name: &str, source: &str) -> Self {
        Self { name: name.to_owned(), source: source.to_owned(), ..Self::default() }
    }

    /// The document as the JSON value `serializeDocument` writes, in the TypeScript key order.
    pub fn to_json(&self) -> Json {
        let mut object = Map::new();
        object.insert("version".into(), Json::from(DOCUMENT_VERSION));
        object.insert("name".into(), Json::String(self.name.clone()));
        object.insert("source".into(), Json::String(self.source.clone()));
        object.insert("overrides".into(), Json::Object(self.overrides.clone()));
        if let Some(materials) = &self.materials {
            object.insert("materials".into(), Json::Array(materials.clone()));
        }
        if let Some(variants) = &self.variants {
            let variants = variants
                .iter()
                .map(|variant| {
                    let mut record = Map::new();
                    record.insert("name".into(), Json::String(variant.name.clone()));
                    record.insert("overrides".into(), Json::Object(variant.overrides.clone()));
                    Json::Object(record)
                })
                .collect();
            object.insert("variants".into(), Json::Array(variants));
        }
        integral_numbers_as_integers(Json::Object(object))
    }
}

/// Validate saved document text before it reaches the editor or compiler (`readDocument`).
pub fn read_document(text: &str) -> Result<FormDocument, DocumentError> {
    if text.len() > DOCUMENT_FILE_BYTES {
        return Err(DocumentError::TooLarge);
    }
    let value: Json = serde_json::from_str(text).map_err(|error| DocumentError::Json(error.to_string()))?;
    read_document_value(&value)
}

/// Validate a parsed document value (`readDocument` after `JSON.parse`).
pub fn read_document_value(value: &Json) -> Result<FormDocument, DocumentError> {
    let version_valid = value.get("version").and_then(Json::as_f64) == Some(DOCUMENT_VERSION as f64);
    let source = value.get("source").and_then(Json::as_str);
    let name = value.get("name").and_then(Json::as_str);
    let (Some(source), Some(name)) = (source, name) else {
        return Err(DocumentError::Invalid);
    };
    if !version_valid || source.chars().count() > MAXIMUM_SOURCE_LENGTH || name.chars().count() > MAXIMUM_NAME_LENGTH {
        return Err(DocumentError::Invalid);
    }
    let overrides = read_overrides(value.get("overrides").unwrap_or(&Json::Object(Map::new())))?;
    let variants = value.get("variants").map(read_variants).transpose()?;
    let materials = value
        .get("materials")
        .map(|materials| read_materials(Some(materials)).map_err(|error| DocumentError::Materials(error.0)))
        .transpose()?
        .map(|materials| materials.into_iter().map(integral_numbers_as_integers).collect());
    Ok(FormDocument { name: name.to_owned(), source: source.to_owned(), overrides, materials, variants })
}

/// Copy and validate overrides at document and variant ownership boundaries (`readOverrides`).
pub fn read_overrides(value: &Json) -> Result<Overrides, DocumentError> {
    let Json::Object(entries) = value else {
        return Err(DocumentError::Overrides(
            "Document parameters must be finite scalars, XYZ vectors, or RGBA colors.".into(),
        ));
    };
    entries
        .iter()
        .map(|(name, value)| {
            read_parameter_value(value)
                .map(|value| (name.clone(), integral_numbers_as_integers(value)))
                .map_err(|error| DocumentError::Overrides(error.0))
        })
        .collect()
}

/// Validate a variant list: at most 16 entries with trimmed, unique, 1–64 character names (`readVariants`).
pub fn read_variants(value: &Json) -> Result<Vec<ParameterVariant>, DocumentError> {
    let Some(entries) = value.as_array().filter(|entries| entries.len() <= MAXIMUM_VARIANTS) else {
        return Err(DocumentError::Variants("A document supports up to 16 parameter variants.".into()));
    };
    let invalid_name = || DocumentError::Variants("Variant names must be unique and contain 1–64 characters.".into());
    let mut names = std::collections::HashSet::new();
    entries
        .iter()
        .map(|entry| {
            let name = entry.get("name").and_then(Json::as_str).ok_or_else(invalid_name)?;
            let trimmed = name.trim();
            if trimmed.is_empty()
                || name.chars().count() > MAXIMUM_VARIANT_NAME_LENGTH
                || !names.insert(trimmed.to_lowercase())
            {
                return Err(invalid_name());
            }
            let overrides = read_overrides(entry.get("overrides").unwrap_or(&Json::Null))?;
            Ok(ParameterVariant { name: trimmed.to_owned(), overrides })
        })
        .collect()
}

/// Serialize a document that reopens; compact JSON avoids whitespace amplification of mesh data
/// (`serializeDocument`). Fails when the result would not read back, for example beyond the 64 MB budget.
pub fn serialize_document(document: &FormDocument) -> Result<String, DocumentError> {
    let text = document.to_json().to_string();
    read_document(&text)?;
    Ok(text)
}

/// A file-name stem from a document name: lowercase ASCII letters and digits joined by hyphens, or `form-study`
/// (`fileStem`).
pub fn file_stem(name: &str) -> String {
    let mut stem = String::new();
    let mut separated = false;
    for character in name.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if separated && !stem.is_empty() {
                stem.push('-');
            }
            separated = false;
            stem.push(character);
        } else {
            separated = true;
        }
    }
    if stem.is_empty() { "form-study".to_owned() } else { stem }
}

#[cfg(test)]
#[path = "document.test.rs"]
mod tests;
