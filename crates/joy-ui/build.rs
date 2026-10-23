// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Compile the UI's editable appearance and layout assets into native and WebAssembly constants.

#[path = "src/theme_build.rs"]
mod theme_build;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let assets = manifest.join("../../assets/ui");
    let theme = assets.join("theme-default.json");
    let layout = assets.join("layout.json");
    println!("cargo:rerun-if-changed={}", theme.display());
    println!("cargo:rerun-if-changed={}", layout.display());

    let source = theme_build::generate(&std::fs::read_to_string(theme)?, &std::fs::read_to_string(layout)?)?;
    let output = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    std::fs::write(output.join("ui_theme.rs"), source)?;
    Ok(())
}
