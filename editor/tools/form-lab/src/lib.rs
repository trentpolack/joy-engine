// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM LAB on the Rust Joy Engine: write FORM scripts and see them through the engine's own renderer, natively and in
//! browsers, standalone or as a Joy Editor tab ([`FormLab`] implements `AuthoringTool`).
//!
//! Model modules carry no UI and sit beside their TypeScript counterparts in `src/`: documents, the Examples library,
//! exports, MaterialX, the compile lifecycle, parameters, the session, the Output inspection model, and the preview
//! (`rendering`). UI modules draw them with the authoring kit's egui widgets: the workbench (`site/src/main.rs`), the
//! inspectors, the dialogs, and viewport input, so a later UI technology change touches only those.

// Model.
pub mod compiler;
pub mod constants;
pub mod document;
pub mod export;
pub mod export_glb;
pub mod form_library;
pub mod materialx;
pub mod output_inspection;
pub mod parameter_values;
pub mod project_metadata;
pub mod rendering;
pub mod script_guide;
pub mod session;

#[path = "../site/src/form_assets.rs"]
pub mod form_assets;

// UI.
pub mod artifact_dialog;
pub mod editor;
pub mod material_controls;
pub mod output_inspector;
pub mod parameter_controls;
pub mod preview_lighting_controls;
pub mod reference_dialog;
pub mod resource_parameter_controls;
pub mod variant_controls;
pub mod workspace_layout;

#[path = "../site/src/main.rs"]
pub mod workbench;

pub use workbench::FormLab;

use joy_engine::runtime::{ShellError, ShellOptions};

/// The shell settings FORM LAB runs with standalone: its title and canvas, a plain studio background (no sky), and
/// the development overlay off.
pub fn shell_options() -> ShellOptions {
    let mut options = ShellOptions {
        title: "FORM LAB".into(),
        canvas_id: "form-lab".into(),
        development: false,
        window_size: (1280, 800),
        ..ShellOptions::default()
    };
    options.renderer.atmosphere = false;
    options
}

/// Run FORM LAB standalone until its window closes (natively) or indefinitely (browsers).
pub fn run() -> Result<(), ShellError> {
    joy_engine::run(shell_options(), FormLab::new())
}

/// The browser entry point.
#[cfg(all(target_arch = "wasm32", feature = "web-entry"))]
fn start() {
    if let Err(error) = run() {
        joy_engine::log::error!("{error}");
    }
}

#[cfg(feature = "web-entry")]
joy_engine::web_main!(start);
