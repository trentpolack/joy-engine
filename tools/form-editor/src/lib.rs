// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM EDITOR on the Rust Joy Engine: write FORM scripts and see them through the engine's own renderer, natively and in
//! browsers, standalone or as a Joy Editor tab ([`FormEditor`] implements `AuthoringTool`).
//!
//! Model modules carry no UI: documents, the Examples library,
//! exports, MaterialX, the compile lifecycle, parameters, the session, the Output inspection model, and the preview
//! (`rendering`). UI modules draw them with the authoring kit's egui widgets: the workbench (`workbench`), the
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

// The engine FORM assets bundled at build time.
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

// The workbench application that composes the model and UI modules.
pub mod workbench;

pub use workbench::FormEditor;

use joy_engine::runtime::{
    ShellError,
    ShellOptions,
};

/// The shell settings FORM EDITOR runs with standalone: its title and canvas, a plain studio background (no sky), and
/// the development overlay off.
pub fn shell_options() -> ShellOptions {
    let mut options = ShellOptions {
        title: "FORM EDITOR".into(),
        canvas_id: "form-editor".into(),
        development: false,
        window_size: (1280, 800),
        ..ShellOptions::default()
    };
    options.renderer.atmosphere = false;
    options
}

/// Run FORM EDITOR standalone until its window closes (natively) or indefinitely (browsers).
pub fn run() -> Result<(), ShellError> {
    joy_engine::run(shell_options(), FormEditor::new())
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
