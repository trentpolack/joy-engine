// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM instance documents (`.forminst`): one configured use of a FORM script.
//!
//! A `.form` file is the recipe: script source and the parameters it declares, and the only thing evaluation runs. A
//! `.forminst` file is an instance of that recipe: which script, the parameter overrides, a material palette, and named
//! variants. The script is normally a reference (`"form": "tower.form"`, relative to the instance's folder) so editing
//! the recipe updates every instance; a packed instance carries its script in `"source"` instead, for sharing one
//! self-contained file. Documents are validated once when read, so editors, hosts, and the compiler only see data
//! that reopens.

use crate::{
    CompiledScript,
    DOCUMENT_FILE_BYTES,
    Diagnostic,
    materials::read_materials,
    modules::{
        MODULE_EXTENSION,
        ModuleLoader,
        resolve_path,
    },
    read_parameter_value,
};
use joy_core::integral_numbers_as_integers;
use serde_json::{
    Map,
    Value as Json,
};

/// File extension of FORM instance documents.
pub const FORM_INSTANCE_EXTENSION: &str = ".forminst";

/// The only instance document version. Version 1 was the `.formlab` document, which always packed its script.
pub const FORM_INSTANCE_VERSION: u64 = 2;

/// Largest packed script, in characters (the FORM source budget).
pub const MAXIMUM_SOURCE_LENGTH: usize = 200_000;

/// Longest instance name, in characters.
pub const MAXIMUM_NAME_LENGTH: usize = 120;

/// Most variants one instance keeps.
pub const MAXIMUM_VARIANTS: usize = 16;

/// Longest variant name, in characters.
pub const MAXIMUM_VARIANT_NAME_LENGTH: usize = 64;

// Document field names, in their stable serialized order.
const FIELD_VERSION: &str = "version";
const FIELD_NAME: &str = "name";
const FIELD_FORM: &str = "form";
const FIELD_SOURCE: &str = "source";
const FIELD_OVERRIDES: &str = "overrides";
const FIELD_MATERIALS: &str = "materials";
const FIELD_VARIANTS: &str = "variants";

/// Parameter overrides keyed by parameter name, in insertion order: numbers, vectors, colors, true/false, text, and
/// mesh or material resources as FORM serializes them.
pub type Overrides = Map<String, Json>;

/// Why an instance document was rejected. Messages are stable diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstanceError {
    /// The text exceeds [`DOCUMENT_FILE_BYTES`].
    #[error("FORM instance documents must be 64 MB or smaller.")]
    TooLarge,
    /// The text is not JSON.
    #[error("This file is not valid JSON: {0}")]
    Json(String),
    /// The JSON is not a version 2 instance with a name and exactly one script within their limits.
    #[error("{0}")]
    Invalid(String),
    /// The overrides are not an object of valid parameter values.
    #[error("{0}")]
    Overrides(String),
    /// The variants are not a list of at most 16 uniquely named override sets.
    #[error("{0}")]
    Variants(String),
    /// The material palette failed FORM's validation.
    #[error("{0}")]
    Materials(String),
}

/// Where an instance's script comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceScript {
    /// A `.form` path relative to the instance's folder, using forward slashes.
    Reference(String),
    /// The script source itself, at most [`MAXIMUM_SOURCE_LENGTH`] characters.
    Packed(String),
}

/// One named set of explicit parameter overrides. Script defaults stay live for parameters it leaves out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParameterVariant {
    /// Trimmed, unique (ignoring case) name of 1–64 characters.
    pub name: String,
    /// The variant's overrides.
    pub overrides: Overrides,
}

/// A validated FORM instance document. The document owns every value; editing a copy never changes another.
#[derive(Clone, Debug, PartialEq)]
pub struct FormInstance {
    /// Display name of at most 120 characters.
    pub name: String,
    /// The referenced or packed script.
    pub script: InstanceScript,
    /// Parameter overrides applied over the script's defaults.
    pub overrides: Overrides,
    /// Validated material palette (Default first), when the instance carries one.
    pub materials: Option<Vec<Json>>,
    /// Named parameter variants, when the instance carries them.
    pub variants: Option<Vec<ParameterVariant>>,
}

impl FormInstance {
    /// A packed instance of `source` without overrides, palette, or variants.
    pub fn packed(name: &str, source: &str) -> Self {
        Self {
            name: name.to_owned(),
            script: InstanceScript::Packed(source.to_owned()),
            overrides: Overrides::new(),
            materials: None,
            variants: None,
        }
    }

    /// An instance referencing the script at `form` (relative to the instance's folder), without overrides, palette,
    /// or variants.
    pub fn referencing(name: &str, form: &str) -> Self {
        Self {
            script: InstanceScript::Reference(form.to_owned()),
            ..Self::packed(name, "")
        }
    }

    /// Validate saved document text.
    pub fn read(text: &str) -> Result<Self, InstanceError> {
        if text.len() > DOCUMENT_FILE_BYTES {
            return Err(InstanceError::TooLarge);
        }
        let value: Json = serde_json::from_str(text).map_err(|error| InstanceError::Json(error.to_string()))?;
        Self::read_value(&value)
    }

    /// Validate a parsed document value.
    pub fn read_value(value: &Json) -> Result<Self, InstanceError> {
        let invalid = |reason: &str| InstanceError::Invalid(format!("This is not a valid FORM instance: {reason}"));
        if value.get(FIELD_VERSION).and_then(Json::as_f64) != Some(FORM_INSTANCE_VERSION as f64) {
            return Err(invalid("it must be document version 2."));
        }
        let name = value
            .get(FIELD_NAME)
            .and_then(Json::as_str)
            .filter(|name| name.chars().count() <= MAXIMUM_NAME_LENGTH)
            .ok_or_else(|| invalid("it needs a name of at most 120 characters."))?;
        let script = match (value.get(FIELD_FORM), value.get(FIELD_SOURCE)) {
            (Some(Json::String(form)), None) => {
                validate_reference(form).map_err(|reason| invalid(&reason))?;
                InstanceScript::Reference(form.clone())
            }
            (None, Some(Json::String(source))) if source.chars().count() <= MAXIMUM_SOURCE_LENGTH => {
                InstanceScript::Packed(source.clone())
            }
            _ => return Err(invalid("it needs either a \"form\" script path or packed \"source\", not both.")),
        };
        let overrides = read_overrides(value.get(FIELD_OVERRIDES).unwrap_or(&Json::Object(Map::new())))?;
        let variants = value.get(FIELD_VARIANTS).map(read_variants).transpose()?;
        let materials = value
            .get(FIELD_MATERIALS)
            .map(|materials| read_materials(Some(materials)).map_err(InstanceError::Materials))
            .transpose()?
            .map(|materials| materials.into_iter().map(integral_numbers_as_integers).collect());
        Ok(Self {
            name: name.to_owned(),
            script,
            overrides,
            materials,
            variants,
        })
    }

    /// The document as the JSON value [`FormInstance::serialize`] writes, with keys in the stable document order.
    pub fn to_json(&self) -> Json {
        let mut object = Map::new();
        object.insert(FIELD_VERSION.into(), Json::from(FORM_INSTANCE_VERSION));
        object.insert(FIELD_NAME.into(), Json::String(self.name.clone()));
        match &self.script {
            InstanceScript::Reference(form) => object.insert(FIELD_FORM.into(), Json::String(form.clone())),
            InstanceScript::Packed(source) => object.insert(FIELD_SOURCE.into(), Json::String(source.clone())),
        };
        object.insert(FIELD_OVERRIDES.into(), Json::Object(self.overrides.clone()));
        if let Some(materials) = &self.materials {
            object.insert(FIELD_MATERIALS.into(), Json::Array(materials.clone()));
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
            object.insert(FIELD_VARIANTS.into(), Json::Array(variants));
        }
        integral_numbers_as_integers(Json::Object(object))
    }

    /// Serialize a document that reopens; compact JSON avoids whitespace amplification of mesh data. Fails when the
    /// result would not read back, for example beyond the 64 MB budget.
    pub fn serialize(&self) -> Result<String, InstanceError> {
        let text = self.to_json().to_string();
        Self::read(&text)?;
        Ok(text)
    }

    /// The referenced script's path relative to the loader's root, given this instance's own path there; `None` for a
    /// packed instance. Fails when the reference leaves the root.
    pub fn script_path(&self, instance_path: &str) -> Result<Option<String>, InstanceError> {
        match &self.script {
            InstanceScript::Packed(_) => Ok(None),
            InstanceScript::Reference(form) => {
                resolve_path(form, Some(&instance_path.replace('\\', "/"))).map(Some).map_err(InstanceError::Invalid)
            }
        }
    }

    /// The overrides for `variant` (matched ignoring case): the document's overrides, then the variant's. `None` uses
    /// the document's overrides alone. Fails when the instance has no variant by that name.
    pub fn overrides_for(&self, variant: Option<&str>) -> Result<Overrides, InstanceError> {
        let mut overrides = self.overrides.clone();
        if let Some(name) = variant {
            let found = self
                .variants
                .iter()
                .flatten()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| InstanceError::Variants(format!("No FORM instance variant named \"{name}\".")))?;
            overrides.extend(found.overrides.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        Ok(overrides)
    }

    /// Parse the instance's script and its modules. `instance_path` is this document's path relative to the loader's
    /// root; the referenced script and every module are read through `loader`.
    pub fn parse_script(&self, instance_path: &str, loader: &dyn ModuleLoader) -> Result<CompiledScript, Diagnostic> {
        let unlocated = |message: String| Diagnostic {
            message,
            line: 1,
            column: 1,
        };
        match &self.script {
            InstanceScript::Packed(source) => CompiledScript::parse_with_modules(source, instance_path, loader),
            InstanceScript::Reference(_) => {
                let path = self
                    .script_path(instance_path)
                    .map_err(|error| unlocated(error.to_string()))?
                    .expect("references resolve to a path");
                let source = loader
                    .load(&path)
                    .map_err(|reason| unlocated(format!("Could not load the instance's script '{path}': {reason}")))?;
                CompiledScript::parse_with_modules(&source, &path, loader)
                    .map_err(|error| unlocated(format!("{path} ({}:{}): {}", error.line, error.column, error.message)))
            }
        }
    }
}

/// Copy and validate overrides at document and variant ownership boundaries.
pub fn read_overrides(value: &Json) -> Result<Overrides, InstanceError> {
    let Json::Object(entries) = value else {
        return Err(InstanceError::Overrides(
            "Document parameters must be numbers, vectors, colors, true/false, text, meshes, or materials.".into(),
        ));
    };
    entries
        .iter()
        .map(|(name, value)| {
            read_parameter_value(value)
                .map(|value| (name.clone(), integral_numbers_as_integers(value)))
                .map_err(|error| InstanceError::Overrides(error.0))
        })
        .collect()
}

/// Validate a variant list: at most 16 entries with trimmed, unique, 1–64 character names.
pub fn read_variants(value: &Json) -> Result<Vec<ParameterVariant>, InstanceError> {
    let Some(entries) = value.as_array().filter(|entries| entries.len() <= MAXIMUM_VARIANTS) else {
        return Err(InstanceError::Variants("A document supports up to 16 parameter variants.".into()));
    };
    let invalid_name = || InstanceError::Variants("Variant names must be unique and contain 1–64 characters.".into());
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
            Ok(ParameterVariant {
                name: trimmed.to_owned(),
                overrides,
            })
        })
        .collect()
}

/// A script reference is a relative `.form` path with forward slashes.
fn validate_reference(form: &str) -> Result<(), String> {
    if form.is_empty() || form.contains('\\') || form.starts_with('/') || form.contains(':') {
        return Err("the \"form\" path must be relative and use forward slashes.".into());
    }
    if !form.ends_with(MODULE_EXTENSION) {
        return Err(format!("the \"form\" path must name a {MODULE_EXTENSION} script."));
    }
    Ok(())
}

#[cfg(test)]
#[path = "instance.test.rs"]
mod tests;
