// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Where the Examples library comes from.
//!
//! Native builds read the engine's `assets/examples` and `assets/form` folders from disk each time the library is
//! refreshed, so new and removed files appear without rebuilding; installed copies without those folders, and browsers,
//! use the text bundled at build time (`build.rs`). Nothing here writes back to an asset.

use crate::document::FormDocument;
use crate::form_library::{FormLibraryAsset, create_form_library, open_form_library_asset};

include!(concat!(env!("OUT_DIR"), "/form_assets.rs"));

/// The study FORM LAB opens first.
pub const DEFAULT_FORM_ASSET_PATH: &str = "assets/examples/wave-study.form";

/// The FORM assets available to this build, in path order.
pub fn form_assets() -> Vec<FormLibraryAsset> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(assets) = disk_assets() {
        return assets;
    }
    bundled_form_assets()
}

/// The assets bundled at build time.
pub fn bundled_form_assets() -> Vec<FormLibraryAsset> {
    create_form_library(BUNDLED_FORM_ASSETS.iter().map(|(path, text)| ((*path).to_owned(), (*text).to_owned())))
}

/// The initial study when available; an edited or empty library still opens an empty document.
pub fn create_initial_form_document(assets: &[FormLibraryAsset]) -> FormDocument {
    assets
        .iter()
        .find(|asset| asset.path == DEFAULT_FORM_ASSET_PATH)
        .or_else(|| assets.first())
        .and_then(|asset| open_form_library_asset(asset).ok())
        .unwrap_or_else(|| FormDocument::from_script("Untitled", ""))
}

/// Read the engine's asset folders from disk, or `None` when this build's checkout is not present.
#[cfg(not(target_arch = "wasm32"))]
fn disk_assets() -> Option<Vec<FormLibraryAsset>> {
    let engine = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut files = Vec::new();
    for folder in ["examples", "form"] {
        let directory = engine.join("assets").join(folder);
        if !directory.is_dir() {
            return None;
        }
        collect(&directory, &format!("assets/{folder}"), &mut files);
    }
    Some(create_form_library(files))
}

/// Add every readable file under `directory`, named relative to the engine root; the library filters extensions.
#[cfg(not(target_arch = "wasm32"))]
fn collect(directory: &std::path::Path, relative: &str, files: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect(&path, &format!("{relative}/{name}"), files);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            files.push((format!("{relative}/{name}"), text));
        }
    }
}
