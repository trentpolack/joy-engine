// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Examples library: authored `.form` and `.formlab` files under the engine's `assets/examples` and `assets/form`
//! folders.
//!
//! The library describes files without evaluating scripts or touching the GPU. Opening an asset parses a fresh working
//! copy each time, so editing, recovery, and saving never change the bundled text or an earlier copy.

use crate::document::{
    DocumentError,
    FormDocument,
    read_document,
};

/// A read-only authored asset, with its path relative to the engine root (for example
/// `assets/examples/wave-study.form`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormLibraryAsset {
    /// Path relative to the engine root, with forward slashes.
    pub path: String,
    /// The containing folder, such as `assets/form/structures`.
    pub folder: String,
    /// The file name with its extension.
    pub filename: String,
    /// The file's text.
    pub text: String,
}

/// Describe the FORM assets among `files` (engine-relative path and text) in path order. Paths outside
/// `assets/examples` and `assets/form`, other extensions, and `..` segments are ignored.
pub fn create_form_library(files: impl IntoIterator<Item = (String, String)>) -> Vec<FormLibraryAsset> {
    let mut assets: Vec<FormLibraryAsset> = files
        .into_iter()
        .filter(|(path, _)| is_library_path(path))
        .map(|(path, text)| {
            let separator = path.rfind('/').unwrap_or(0);
            FormLibraryAsset {
                folder: path[..separator].to_owned(),
                filename: path[separator + 1..].to_owned(),
                path,
                text,
            }
        })
        .collect();
    // `localeCompare` orders case-insensitively first; ties fall back to the exact text.
    assets.sort_by(|first, second| {
        first.path.to_lowercase().cmp(&second.path.to_lowercase()).then_with(|| first.path.cmp(&second.path))
    });
    assets
}

/// Whether an engine-relative path names a FORM library asset.
pub fn is_library_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let rest = lower.strip_prefix("assets/examples/").or_else(|| lower.strip_prefix("assets/form/"));
    let named = rest.is_some_and(|rest| {
        let stem = rest.strip_suffix(".formlab").or_else(|| rest.strip_suffix(".form"));
        stem.is_some_and(|stem| !stem.is_empty())
    });
    named && !path.split('/').any(|segment| segment == "..")
}

/// Parse a fresh, independent working copy of an asset. `.form` files become a document named after the file
/// (hyphens and underscores read as spaces); `.formlab` files keep their overrides, materials, meshes, and variants.
pub fn open_form_library_asset(asset: &FormLibraryAsset) -> Result<FormDocument, DocumentError> {
    if asset.filename.to_ascii_lowercase().ends_with(".formlab") {
        return read_document(&asset.text);
    }
    let stem = match asset.filename.len().checked_sub(5) {
        Some(end) if asset.filename[end..].eq_ignore_ascii_case(".form") => &asset.filename[..end],
        _ => asset.filename.as_str(),
    };
    let name = stem.replace(['-', '_'], " ");
    // Reading through the document validator applies the same source and name limits as saved documents.
    read_document(&FormDocument::from_script(&name, &asset.text).to_json().to_string())
}
