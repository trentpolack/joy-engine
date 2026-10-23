// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The development editors every game attaches: the post-processing editor for the level's global volume and, when a
//! game has tuning, the TUNE panel. Games describe what to edit once; [`DevEditors`] creates both editors the first
//! frame development tools are on, draws them, and resolves post-processing through the editor's preview.
//!
//! Saves go to the game's source checkout only in native development builds ([`development_file`]); browser and
//! release builds keep edits for the session.

use std::path::PathBuf;

use joy_core::math::Vec3;
use joy_core::{
    ConfigField,
    create_config_document_values,
};
use joy_renderer::PostProcessSettings;
use joy_scene::PostProcessVolumes;

use crate::config_panel::{
    ConfigEditor,
    ConfigEditorOptions,
};
use crate::post_process_editor::{
    PostProcessEditor,
    PostProcessEditorOptions,
};

/// The TUNE panel a game offers: its fields, shipped override document, and development save path.
pub struct TuningPanel {
    /// Panel title.
    pub title: String,
    /// The tuning fields.
    pub fields: Vec<ConfigField>,
    /// The shipped override document (JSON text, usually `include_str!` of `config/tuning-overrides.json`).
    pub document: &'static str,
    /// Where Save writes in native development builds; see [`development_file`].
    pub file: Option<PathBuf>,
    /// Label for a restart button, for games with `restart` apply-mode fields.
    pub restart_label: Option<String>,
}

/// What a game's development editors edit.
pub struct DevEditorsOptions {
    /// Game title shown in the panels.
    pub title: String,
    /// The authored level text whose global `postProcess` volume is edited.
    pub level_source: &'static str,
    /// Where post-processing saves in native development builds; see [`development_file`].
    pub level_file: Option<PathBuf>,
    /// Whether P toggles the post-processing preview pause (games that bind P turn this off).
    pub pause_shortcut: bool,
    /// The TUNE panel, for games with tuning fields.
    pub tuning: Option<TuningPanel>,
}

/// A game's development editors, created lazily the first frame development tools are enabled.
#[derive(Default)]
pub struct DevEditors {
    post: Option<PostProcessEditor>,
    config: Option<ConfigEditor>,
    attempted: bool,
}

impl DevEditors {
    /// No editors yet; [`attach`](Self::attach) creates them.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create the editors once, the first time `enabled` (usually `frame.dev_tools.enabled()`) is true; `options` runs
    /// at most once. An editor that cannot start is logged and left out, so the game keeps running.
    pub fn attach(&mut self, enabled: bool, options: impl FnOnce() -> DevEditorsOptions) {
        if !enabled || std::mem::replace(&mut self.attempted, true) {
            return;
        }
        let options = options();
        self.post = post_process_editor(&options);
        self.config = options.tuning.and_then(|tuning| config_editor(&options.title, tuning));
    }

    /// Draw the editors' launchers and panels.
    pub fn ui(&mut self, context: &egui::Context) {
        if let Some(editor) = &mut self.post {
            editor.ui(context);
        }
        if let Some(editor) = &mut self.config {
            editor.ui(context);
        }
    }

    /// The post-processing settings at `view_point`: the editor's preview when it exists, otherwise the level's volumes.
    pub fn resolve(&self, volumes: &mut PostProcessVolumes, view_point: Vec3) -> PostProcessSettings {
        PostProcessEditor::resolve(self.post.as_ref(), volumes, view_point)
    }

    /// Whether the post-processing editor paused the preview.
    pub fn paused(&self) -> bool {
        self.post.as_ref().is_some_and(PostProcessEditor::paused)
    }

    /// The post-processing editor, once attached.
    pub fn post_editor(&self) -> Option<&PostProcessEditor> {
        self.post.as_ref()
    }

    /// The post-processing editor, mutably.
    pub fn post_editor_mut(&mut self) -> Option<&mut PostProcessEditor> {
        self.post.as_mut()
    }

    /// The TUNE editor, once attached.
    pub fn config_editor(&self) -> Option<&ConfigEditor> {
        self.config.as_ref()
    }

    /// The TUNE editor, mutably (for applying changes, presets, and restarts).
    pub fn config_editor_mut(&mut self) -> Option<&mut ConfigEditor> {
        self.config.as_mut()
    }
}

/// `path` (an absolute source-checkout path, usually `concat!(env!("CARGO_MANIFEST_DIR"), "/assets/…")`) in native
/// development builds, where editors save; `None` in browser and release builds, where edits stay in the session.
pub fn development_file(path: &str) -> Option<PathBuf> {
    if cfg!(all(debug_assertions, not(target_arch = "wasm32"))) { Some(PathBuf::from(path)) } else { None }
}

/// The level's post-processing editor, or `None` (logged) when the level has no editable volume.
fn post_process_editor(options: &DevEditorsOptions) -> Option<PostProcessEditor> {
    let level = match serde_json::from_str(options.level_source) {
        Ok(level) => level,
        Err(error) => {
            log::warn!("Post-processing editor unavailable: the level is not JSON: {error}");
            return None;
        }
    };
    let editor = PostProcessEditor::new(PostProcessEditorOptions {
        title: options.title.clone(),
        level,
        file: options.level_file.clone(),
        pause_shortcut: options.pause_shortcut,
        ..PostProcessEditorOptions::default()
    });
    match editor {
        Ok(editor) => Some(editor),
        Err(error) => {
            log::warn!("Post-processing editor unavailable: {error}");
            None
        }
    }
}

/// The TUNE editor, or `None` (logged) when the shipped document or fields are invalid.
fn config_editor(title: &str, tuning: TuningPanel) -> Option<ConfigEditor> {
    let document = match serde_json::from_str(tuning.document) {
        Ok(document) => document,
        Err(error) => {
            log::warn!("Config editor unavailable for {title}: the tuning document is not JSON: {error}");
            return None;
        }
    };
    let values = match create_config_document_values(&tuning.fields, &document) {
        Ok(values) => values,
        Err(error) => {
            log::warn!("Config editor unavailable for {title}: {error}");
            return None;
        }
    };
    let editor = ConfigEditor::new(ConfigEditorOptions {
        title: tuning.title,
        fields: tuning.fields,
        values,
        file: tuning.file,
        restart_label: tuning.restart_label,
    });
    match editor {
        Ok(editor) => Some(editor),
        Err(error) => {
            log::warn!("Config editor unavailable for {title}: {error}");
            None
        }
    }
}
