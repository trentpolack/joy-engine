// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Project-owned editor manifests (`project.joyproject`), ported from the TypeScript `project-manifest.ts`.
//!
//! Parsing validates and copies the manifest: required text is trimmed, paths are normalized to forward slashes and
//! must stay inside the selected project folder, capability ids are unique kebab-case, extensions are lowercase dot
//! notation, and feature flags become booleans by JavaScript truthiness. Paths stay relative to the project directory;
//! consumers must still enforce that boundary when reading. The optional `$schema` reference is preserved for
//! authoring tools and never fetched. Messages match the TypeScript engine.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The manifest filename shared by browser directory handles and the desktop host.
pub const JOY_PROJECT_MANIFEST: &str = "project.joyproject";

/// The only supported manifest schema version.
pub const PROJECT_SCHEMA_VERSION: u32 = 1;

/// Where a project's editor preview is served from, relative to the project folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPreview {
    /// Folder holding the built preview, for example `dist`.
    pub root: String,
    /// Entry document inside `root`, for example `index.html`.
    pub entry: String,
}

/// One editor capability: which asset files a project authors and which renderer previews them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCapability {
    /// Unique kebab-case id, for example `particle-effects`.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Lowercase dot-notation extensions, for example `.joyfx`; at least one.
    pub extensions: Vec<String>,
    /// Folder holding the capability's assets, relative to the project folder.
    pub asset_root: String,
    /// Renderer id the editor previews these assets with.
    pub renderer: String,
    /// Feature flags in authored order.
    pub features: IndexMap<String, bool>,
}

/// A validated project manifest. Serializes in the TypeScript key order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    /// Optional JSON Schema reference for authoring tools (serialized as `$schema` when present).
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Always [`PROJECT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Trimmed project name.
    pub name: String,
    /// Preview location.
    pub preview: ProjectPreview,
    /// Editor capabilities in authored order.
    pub capabilities: Vec<ProjectCapability>,
}

/// A manifest failed validation. Messages match the TypeScript engine.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProjectManifestError {
    /// The text was not valid JSON.
    #[error("Invalid project manifest JSON: {0}")]
    Json(String),
    /// The manifest was not an object, or its schema version was not 1.
    #[error("Joy project manifest must use schemaVersion 1.")]
    UnsupportedVersion,
    /// A required text field was missing, not a string, or blank; the payload names it.
    #[error("Joy project manifest {0} must be a non-empty string.")]
    MissingText(String),
    /// `preview` was not an object.
    #[error("Joy project manifest requires a preview object.")]
    MissingPreview,
    /// `capabilities` was not an array.
    #[error("Joy project manifest capabilities must be an array.")]
    InvalidCapabilities,
    /// A capability entry was not an object; the payload is its one-based position.
    #[error("Capability {0} must be an object.")]
    InvalidCapability(usize),
    /// A capability id was not kebab-case or repeated an earlier id.
    #[error("Capability id \"{0}\" must be unique kebab-case.")]
    InvalidCapabilityId(String),
    /// A capability listed no extensions.
    #[error("Capability \"{0}\" requires at least one extension.")]
    MissingExtensions(String),
    /// An extension was not a dot followed by lowercase letters and digits.
    #[error("Capability \"{capability}\" has invalid extension \"{extension}\".")]
    InvalidExtension {
        /// Capability id.
        capability: String,
        /// The normalized (lowercase) extension.
        extension: String,
    },
    /// A path was absolute, used a drive letter, or contained a `..` segment.
    #[error("Project path must stay inside the selected folder: {0}")]
    UnsafePath(String),
}

/// Parse manifest text and validate it with [`parse_project_manifest`].
pub fn parse_project_manifest_text(text: &str) -> Result<ProjectManifest, ProjectManifestError> {
    let value: Value = serde_json::from_str(text).map_err(|error| ProjectManifestError::Json(error.to_string()))?;
    parse_project_manifest(&value)
}

/// Validate and copy a parsed (untrusted) project manifest.
pub fn parse_project_manifest(value: &Value) -> Result<ProjectManifest, ProjectManifestError> {
    let record = match value {
        Value::Object(record) if record.get("schemaVersion").and_then(Value::as_f64) == Some(1.0) => record,
        _ => return Err(ProjectManifestError::UnsupportedVersion),
    };
    let name = required_text(record.get("name"), "name")?;
    let schema = match record.get("$schema") {
        None => None,
        Some(schema) => Some(required_text(Some(schema), "$schema")?),
    };

    let Some(Value::Object(preview)) = record.get("preview") else {
        return Err(ProjectManifestError::MissingPreview);
    };
    let preview = ProjectPreview {
        root: safe_relative_path(&required_text(preview.get("root"), "preview.root")?)?,
        entry: safe_relative_path(&required_text(preview.get("entry"), "preview.entry")?)?,
    };

    let Some(Value::Array(items)) = record.get("capabilities") else {
        return Err(ProjectManifestError::InvalidCapabilities);
    };
    let mut capabilities: Vec<ProjectCapability> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let capability = parse_capability(item, index, &capabilities)?;
        capabilities.push(capability);
    }

    Ok(ProjectManifest { schema, schema_version: PROJECT_SCHEMA_VERSION, name, preview, capabilities })
}

/// Validate one capability entry against those already accepted.
fn parse_capability(
    item: &Value,
    index: usize,
    accepted: &[ProjectCapability],
) -> Result<ProjectCapability, ProjectManifestError> {
    let Value::Object(item) = item else {
        return Err(ProjectManifestError::InvalidCapability(index + 1));
    };
    let id = required_text(item.get("id"), &format!("capabilities[{index}].id"))?;
    if !is_identifier(&id) || accepted.iter().any(|capability| capability.id == id) {
        return Err(ProjectManifestError::InvalidCapabilityId(id));
    }

    let extensions = match item.get("extensions") {
        Some(Value::Array(extensions)) if !extensions.is_empty() => {
            extensions.iter().map(|extension| normalize_extension(extension, &id)).collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(ProjectManifestError::MissingExtensions(id)),
    };
    let asset_root = safe_relative_path(&required_text(item.get("assetRoot"), &format!("capability {id} assetRoot"))?)?;
    let renderer = required_text(item.get("renderer"), &format!("capability {id} renderer"))?;
    let features = match item.get("features") {
        Some(Value::Object(features)) => read_features(features),
        _ => IndexMap::new(),
    };
    let label = required_text(item.get("label"), &format!("capability {id} label"))?;
    Ok(ProjectCapability { id, label, extensions, asset_root, renderer, features })
}

/// Trim required text, reporting the field when it is missing, not a string, or blank.
fn required_text(value: Option<&Value>, field: &str) -> Result<String, ProjectManifestError> {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(text.trim().to_owned()),
        _ => Err(ProjectManifestError::MissingText(field.to_owned())),
    }
}

/// Normalize separators and reject absolute paths, drive letters, and traversal before host resolution.
fn safe_relative_path(value: &str) -> Result<String, ProjectManifestError> {
    let path = value.replace('\\', "/");
    let bytes = path.as_bytes();
    let drive_letter = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.starts_with('/') || path.split('/').any(|part| part == "..") || drive_letter {
        return Err(ProjectManifestError::UnsafePath(value.to_owned()));
    }
    Ok(path.strip_prefix("./").unwrap_or(&path).to_owned())
}

/// Normalize a capability extension to lowercase dot notation.
fn normalize_extension(value: &Value, id: &str) -> Result<String, ProjectManifestError> {
    let extension = required_text(Some(value), &format!("capability {id} extension"))?.to_lowercase();
    let valid = extension.strip_prefix('.').is_some_and(|rest| {
        !rest.is_empty() && rest.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
    });
    if !valid {
        return Err(ProjectManifestError::InvalidExtension { capability: id.to_owned(), extension });
    }
    Ok(extension)
}

/// Kebab-case identifier: a lowercase letter followed by lowercase letters, digits, and hyphens.
fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-')
}

/// Feature flags by JavaScript truthiness, in authored order.
fn read_features(features: &Map<String, Value>) -> IndexMap<String, bool> {
    features.iter().map(|(feature, enabled)| (feature.clone(), is_truthy(enabled))).collect()
}

/// JavaScript `Boolean(value)` for JSON values.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_normalize_and_reject_escapes() {
        assert_eq!(safe_relative_path("./dist\\site").unwrap(), "dist/site");
        assert!(safe_relative_path("/abs").is_err());
        assert!(safe_relative_path("C:/games").is_err());
        assert!(safe_relative_path("assets/../../x").is_err());
        assert!(is_identifier("particle-effects"));
        assert!(!is_identifier("Particle"));
        assert!(!is_identifier("1st"));
    }

    #[test]
    fn minimal_manifests_omit_schema() {
        let minimal = json!({"schemaVersion": 1, "name": " Test ", "preview": {"root": "dist", "entry": "index.html"}, "capabilities": []});
        let manifest = parse_project_manifest(&minimal).unwrap();
        assert_eq!(manifest.name, "Test");
        let value = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            value.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["schemaVersion", "name", "preview", "capabilities"]
        );
    }
}
