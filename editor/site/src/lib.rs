// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Editor on the Rust engine: one workspace for a game project's levels, reusable objects, FORM meshes, and
//! particle effects.
//!
//! Module responsibilities:
//! - [`workspace`] (`site/src/workspace/`): project selection, the Content Browser, document tabs, saves with revision
//!   checks and conflict resolution, session restore, repair views, and hosting FORM LAB and PARTICLE LAB tabs through
//!   `joy_engine::development::AuthoringTool`.
//! - [`level`] (`site/src/level/`): the level and object editor (viewport, outliner, schema-driven inspector).
//! - [`interaction`] (`site/src/interaction/`): camera responses to the kit's viewport gestures, inspector drafts, and
//!   shared widgets.
//! - [`server`] (`server/`): in-process project file access.
//! - [`desktop`] (`desktop/`): the native host.
//!
//! Document and model logic stays in modules without egui (`workspace::main`, `level::main`, `level::property_editor`,
//! `level::viewport`, `server`); egui code lives in the `ui` modules and `interaction::widgets`, so a later UI
//! technology change touches only them. [`JoyEditor`] ties them into a `joy_engine::Game`.
//!
//! The browser build opens read-only bundled examples ([`server::BundledProjects`]): a static page has no project file
//! service.

#[path = "../../desktop/mod.rs"]
pub mod desktop;
#[path = "../../server/mod.rs"]
pub mod server;

pub mod editor;
pub mod interaction;
pub mod level;
pub mod workspace;

pub use editor::{EditorOptions, JoyEditor};

use joy_engine::rendering::RendererOptions;
use joy_engine::runtime::ShellOptions;

use crate::workspace::constants::STORAGE_APPLICATION;
use crate::workspace::tool_bridge::ToolRegistry;

/// The tools Joy Editor hosts as tabs.
///
/// FORM LAB opens `.form` and `.formlab` documents and PARTICLE LAB opens `.joyfx`; formats without a registered tool
/// open in a source view.
pub fn default_tools() -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(|| Box::new(form_lab::FormLab::hosted()));
    tools.register(|| Box::new(particle_lab::ParticleLabTool::with_storage(None)));
    tools
}

/// Window and renderer settings for the editor.
pub fn shell_options() -> ShellOptions {
    ShellOptions {
        title: "Joy Editor".to_owned(),
        development: false,
        renderer: RendererOptions::default(),
        window_size: (1680, 1050),
        canvas_id: "joy-editor".to_owned(),
        ..Default::default()
    }
}

/// Browser entry: the read-only bundled examples with a session in `localStorage`.
pub fn start_browser() {
    let options = EditorOptions {
        store: Box::new(server::BundledProjects::new()),
        storage: Some(joy_engine::runtime::Storage::new(STORAGE_APPLICATION)),
        tools: default_tools(),
        project: None,
        startup_message: Some(
            "Browser build: bundled examples are read-only. Run the native Joy Editor to edit project files."
                .to_owned(),
        ),
    };
    if let Err(error) = joy_engine::run(shell_options(), JoyEditor::new(options)) {
        joy_engine::log::error!("{error}");
    }
}

joy_engine::web_main!(start_browser);
