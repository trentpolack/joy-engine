// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Bundles the engine's authored FORM assets (`assets/examples` and `assets/form`, recursively) for builds that cannot
//! read them from disk, such as browsers: writes `form_assets.rs` into `OUT_DIR` with one `include_str!` per file.
//! Adding or removing a file rebuilds the list.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let engine = manifest.join("../../..");
    let mut files = Vec::new();
    for folder in ["examples", "form"] {
        let directory = engine.join("assets").join(folder);
        println!("cargo:rerun-if-changed={}", directory.display());
        collect(&directory, &format!("assets/{folder}"), &mut files);
    }
    files.sort();
    let mut source = String::from(
        "/// Engine-relative paths and texts of the FORM assets bundled at build time.\n\
         pub const BUNDLED_FORM_ASSETS: &[(&str, &str)] = &[\n",
    );
    for (relative, absolute) in &files {
        source.push_str(&format!("    ({relative:?}, include_str!({:?})),\n", absolute.display().to_string()));
    }
    source.push_str("];\n");
    let output = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("form_assets.rs");
    std::fs::write(output, source).expect("bundled asset list is written");
}

/// Add every `.form` and `.formlab` file under `directory`, named relative to the engine root.
fn collect(directory: &Path, relative: &str, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect(&path, &format!("{relative}/{name}"), files);
        } else {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".form") || lower.ends_with(".formlab") {
                let absolute = path.canonicalize().unwrap_or(path);
                files.push((format!("{relative}/{name}"), absolute));
            }
        }
    }
}
