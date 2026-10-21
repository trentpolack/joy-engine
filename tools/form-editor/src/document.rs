// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM EDITOR's working document: a FORM script and the instance data edited beside it (parameter overrides, a
//! material palette, and named variants).
//!
//! On disk the document is either a `.form` script alone or a `.forminst` FORM instance (see
//! [`joy_engine::form::FormInstance`]), which the engine owns and validates. An instance usually refers to its script
//! by path; FORM EDITOR then loads that script into [`FormDocument::source`] and remembers the reference in
//! [`FormDocument::script_path`], so saving writes the reference rather than a copy. Packed instances carry their
//! script and keep `script_path` empty. Version 1 `.formlab` documents open as packed instances.

use joy_engine::core::integral_numbers_as_integers;
use joy_engine::form::{
    DOCUMENT_FILE_BYTES,
    FORM_INSTANCE_VERSION,
    FormInstance,
    InstanceScript,
};
use joy_engine::serde_json::{
    self,
    Value as Json,
};

pub use joy_engine::form::{
    InstanceError as DocumentError,
    MAXIMUM_NAME_LENGTH,
    MAXIMUM_SOURCE_LENGTH,
    MAXIMUM_VARIANT_NAME_LENGTH,
    MAXIMUM_VARIANTS,
    Overrides,
    ParameterVariant,
    read_overrides,
    read_variants,
};

/// Version of the `.formlab` documents FORM EDITOR still opens (and saves as `.forminst`).
const LEGACY_DOCUMENT_VERSION: u64 = 1;

/// Reads a project file by its path relative to the reader's root, returning its text or a short reason it could not
/// be read.
pub type ScriptReader<'a> = dyn Fn(&str) -> Result<String, String> + 'a;

/// A validated working document. The document owns every value; editing a copy never changes another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormDocument {
    /// Display name of at most 120 characters.
    pub name: String,
    /// FORM script source of at most 200,000 characters: the instance's packed script, or its referenced script as
    /// loaded.
    pub source: String,
    /// The path a referencing instance names for its script, relative to the instance's folder; `None` for scripts
    /// and packed instances.
    pub script_path: Option<String>,
    /// Parameter overrides applied over the script's defaults.
    pub overrides: Overrides,
    /// Validated material palette (Default first), when the document carries one.
    pub materials: Option<Vec<Json>>,
    /// Named parameter variants, when the document carries them.
    pub variants: Option<Vec<ParameterVariant>>,
}

impl FormDocument {
    /// A document for raw script source, as FORM EDITOR opens `.form` files: no overrides, palette, or variants.
    pub fn from_script(name: &str, source: &str) -> Self {
        Self {
            name: name.to_owned(),
            source: source.to_owned(),
            ..Self::default()
        }
    }

    /// The document as a FORM instance: referencing its script when it came from a referencing instance, packed
    /// otherwise.
    pub fn to_instance(&self) -> FormInstance {
        let script = match &self.script_path {
            Some(path) => InstanceScript::Reference(path.clone()),
            None => InstanceScript::Packed(self.source.clone()),
        };
        FormInstance {
            name: self.name.clone(),
            script,
            overrides: self.overrides.clone(),
            materials: self.materials.clone(),
            variants: self.variants.clone(),
        }
    }

    /// The document as the JSON value [`serialize_document`] writes, with keys in the stable document order.
    pub fn to_json(&self) -> Json {
        self.to_instance().to_json()
    }

    /// The document with its script packed, as browser recovery stores it (recovery must not depend on another file).
    pub fn packed(&self) -> Self {
        Self {
            script_path: None,
            ..self.clone()
        }
    }
}

/// Validate saved instance text that packs its script (or a version 1 `.formlab` document) before it reaches the
/// editor or compiler. A referencing instance needs [`read_instance_document`], which can load its script.
pub fn read_document(text: &str) -> Result<FormDocument, DocumentError> {
    read_instance_document(text, "", None)
}

/// Validate saved instance text, loading a referenced script through `read` relative to `path` (the instance's own
/// path, relative to the reader's root).
pub fn read_instance_document(
    text: &str,
    path: &str,
    read: Option<&ScriptReader<'_>>,
) -> Result<FormDocument, DocumentError> {
    if text.len() > DOCUMENT_FILE_BYTES {
        return Err(DocumentError::TooLarge);
    }
    let mut value: Json = serde_json::from_str(text).map_err(|error| DocumentError::Json(error.to_string()))?;
    // A version 1 `.formlab` document always packed its script; open it as a version 2 packed instance.
    if value.get("version").and_then(Json::as_u64) == Some(LEGACY_DOCUMENT_VERSION) && value.get("source").is_some() {
        value["version"] = Json::from(FORM_INSTANCE_VERSION);
    }
    let instance = FormInstance::read_value(&value)?;
    let (source, script_path) = match &instance.script {
        InstanceScript::Packed(source) => (source.clone(), None),
        InstanceScript::Reference(reference) => {
            // Without a project to read from, explain how to open the instance before judging its path, which only
            // resolves against a project folder.
            let Some(read) = read else {
                return Err(DocumentError::Invalid(format!(
                    "This instance uses the script {reference}. Open it from its project folder so FORM EDITOR can read the script, or save it as a packed instance."
                )));
            };
            let resolved = instance.script_path(path)?.unwrap_or_default();
            let source = read(&resolved).map_err(|reason| {
                DocumentError::Invalid(format!("Cannot read this instance's script {reference}: {reason}"))
            })?;
            (source, Some(reference.clone()))
        }
    };
    if source.chars().count() > MAXIMUM_SOURCE_LENGTH {
        return Err(DocumentError::Invalid("Script size limit is 200,000 characters.".into()));
    }
    Ok(FormDocument {
        name: instance.name,
        source,
        script_path,
        overrides: instance.overrides,
        materials: instance
            .materials
            .map(|materials| materials.into_iter().map(integral_numbers_as_integers).collect()),
        variants: instance.variants,
    })
}

/// Serialize a document that reopens; compact JSON avoids whitespace amplification of mesh data. Fails when the result
/// would not read back, for example beyond the 64 MB budget.
pub fn serialize_document(document: &FormDocument) -> Result<String, DocumentError> {
    document.to_instance().serialize()
}

/// A file-name stem from a document name: lowercase ASCII letters and digits joined by hyphens, or `form-study`.
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
