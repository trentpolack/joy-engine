// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Examples library: authored `.form` scripts and `.forminst` instances under the engine's `assets/examples` and
//! `assets/form` folders. Library scripts can `use` other library scripts, and instances refer to library scripts.
//!
//! The library describes files without evaluating scripts or touching the GPU. Opening an asset parses a fresh working
//! copy each time, so editing, recovery, and saving never change the bundled text or an earlier copy.

use std::collections::HashMap;
use std::sync::Arc;

use joy_engine::development::ToolFileReader;

use crate::document::{
    DocumentError,
    FormDocument,
    read_document,
    read_instance_document,
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
        let stem = rest.strip_suffix(".forminst").or_else(|| rest.strip_suffix(".form"));
        stem.is_some_and(|stem| !stem.is_empty())
    });
    named && !path.split('/').any(|segment| segment == "..")
}

/// A reader over the library's own files, by engine-relative path, for the scripts instances refer to and the
/// modules scripts use.
pub fn library_reader(assets: &[FormLibraryAsset]) -> ToolFileReader {
    let files: HashMap<String, String> = assets.iter().map(|asset| (asset.path.clone(), asset.text.clone())).collect();
    Arc::new(move |path: &str| files.get(path).cloned().ok_or_else(|| format!("{path} is not in the library")))
}

/// Parse a fresh, independent working copy of an asset. `.form` files become a document named after the file
/// (hyphens and underscores read as spaces); `.forminst` files keep their overrides, materials, meshes, and variants,
/// and load the library script they refer to.
pub fn open_form_library_asset(
    asset: &FormLibraryAsset,
    assets: &[FormLibraryAsset],
) -> Result<FormDocument, DocumentError> {
    if asset.filename.to_ascii_lowercase().ends_with(".forminst") {
        let reader = library_reader(assets);
        let read = |path: &str| reader(path);
        return read_instance_document(&asset.text, &asset.path, Some(&read));
    }
    let stem = match asset.filename.len().checked_sub(5) {
        Some(end) if asset.filename[end..].eq_ignore_ascii_case(".form") => &asset.filename[..end],
        _ => asset.filename.as_str(),
    };
    let name = stem.replace(['-', '_'], " ");
    // Reading through the document validator applies the same source and name limits as saved documents.
    read_document(&FormDocument::from_script(&name, &asset.text).to_json().to_string())
}
