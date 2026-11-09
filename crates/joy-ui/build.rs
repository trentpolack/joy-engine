// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Compile the UI's editable appearance and layout assets into native and WebAssembly constants.

#[path = "src/theme_build.rs"]
mod theme_build;

/// The editor palettes beside the default theme, in menu order after Joy.
const PALETTE_FILES: [&str; 2] = ["theme-neutral-dark.json", "theme-neutral-light.json"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let assets = manifest.join("../../assets/ui");
    let theme = assets.join("theme-default.json");
    let layout = assets.join("layout.json");
    println!("cargo:rerun-if-changed={}", theme.display());
    println!("cargo:rerun-if-changed={}", layout.display());
    let mut palettes = Vec::new();
    for file in PALETTE_FILES {
        let path = assets.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        palettes.push((file, std::fs::read_to_string(path)?));
    }
    let palettes: Vec<(&str, &str)> = palettes.iter().map(|(file, text)| (*file, text.as_str())).collect();

    let source = theme_build::generate(&std::fs::read_to_string(theme)?, &std::fs::read_to_string(layout)?, &palettes)?;
    let output = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    std::fs::write(output.join("ui_theme.rs"), source)?;
    Ok(())
}
