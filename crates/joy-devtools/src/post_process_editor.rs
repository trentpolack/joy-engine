// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The in-game post-processing editor: live tuning of one post-process volume's settings (by default the level's
//! global, unbounded volume) with undo, preview modes, and saving back into the level.
//!
//! A game owns the editor in development builds, draws it from its UI phase, and resolves post-processing through
//! [`preview`](PostProcessEditor::preview) instead of [`PostProcessVolumes::resolve`] while it exists. Native builds
//! given the level's path save into the volume's `postProcess` component and pick up outside edits; browser builds have
//! no file access, so Export copies the settings to the clipboard instead.

use std::path::PathBuf;

use egui::{
    Context,
    RichText,
};
use joy_core::math::Vec3;
use joy_gameplay::component_type;
use joy_renderer::post_process::{
    POST_PROCESS_CONTROLS,
    PostProcessControlDefault,
};
use joy_renderer::{
    PostProcessError,
    PostProcessSettings,
};
use joy_scene::{
    PostProcessComponent,
    PostProcessVolumes,
};
use serde_json::Value;

use crate::post_process_session::{
    PostProcessSession,
    PostProcessValue,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::volume_file::{
    SaveError,
    VolumeFile,
};
use crate::{
    dock,
    dock::DockTool,
};

/// Largest settings record accepted by Import.
const MAXIMUM_IMPORT_BYTES: usize = 16 * 1024;

/// Editor configuration.
#[derive(Clone, Debug)]
pub struct PostProcessEditorOptions {
    /// Game title shown in the panel.
    pub title: String,
    /// The level document (as JSON) whose volume is edited.
    pub level: Value,
    /// The entity whose `postProcess` volume to edit; `None` edits the level's first enabled unbounded volume.
    pub entity: Option<String>,
    /// Native development builds: the level document's path. Save writes the volume there and outside edits reload.
    /// Ignored in browsers.
    pub file: Option<PathBuf>,
    /// Whether the P key toggles the preview pause. Games that bind P themselves turn this off.
    pub pause_shortcut: bool,
}

impl Default for PostProcessEditorOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            level: Value::Null,
            entity: None,
            file: None,
            pause_shortcut: true,
        }
    }
}

/// Why an editor could not start.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PostProcessEditorError {
    /// The level has no volume to edit: no entity with an enabled, unbounded `postProcess` component, or the named
    /// entity has no `postProcess` component.
    #[error("The level has no post-process volume to edit{0}.")]
    NoVolume(String),
    /// The volume's component record is invalid.
    #[error("Entity {entity}: {message}")]
    InvalidVolume {
        /// The volume's entity.
        entity: String,
        /// What is wrong with it.
        message: String,
    },
}

/// One user request from the panel, applied after drawing.
enum Action {
    Set(&'static str, PostProcessValue),
    Reset(&'static str),
    Undo,
    Redo,
    Revert,
    Save,
    UseDisk,
    KeepLocal,
    Export,
    Import,
    Paused(bool),
    CompareSaved(bool),
    Bypass(bool),
}

/// Live tuning of one post-process volume.
pub struct PostProcessEditor {
    title: String,
    entity: String,
    session: PostProcessSession,
    pause_shortcut: bool,
    paused: bool,
    compare_saved: bool,
    bypassed: bool,
    error: String,
    notice: String,
    clock: f64,
    import_open: bool,
    import_text: String,
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<VolumeFile>,
}

impl PostProcessEditor {
    /// Find and validate the volume to edit and, natively, load the level document.
    pub fn new(options: PostProcessEditorOptions) -> Result<Self, PostProcessEditorError> {
        let (entity, component) = find_volume(&options.level, options.entity.as_deref())?;
        let editor = Self {
            title: options.title,
            session: PostProcessSession::new(component.overrides, ""),
            pause_shortcut: options.pause_shortcut,
            paused: false,
            compare_saved: false,
            bypassed: false,
            error: String::new(),
            notice: String::new(),
            clock: 0.0,
            import_open: false,
            import_text: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            file: options.file.map(|path| VolumeFile::new(path, entity.clone())),
            entity,
        };
        #[cfg(not(target_arch = "wasm32"))]
        let mut editor = editor;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(file) = &mut editor.file {
            match file.read() {
                Ok(snapshot) => editor.session.receive_external(snapshot.overrides, &snapshot.revision),
                Err(error) => {
                    editor.error = format!("Cannot load the level: {error}");
                    editor.file = None;
                }
            }
        }
        Ok(editor)
    }

    /// The entity whose volume is edited.
    pub fn entity(&self) -> &str {
        &self.entity
    }

    /// The editing session, for tests and tools.
    pub fn session(&self) -> &PostProcessSession {
        &self.session
    }

    /// Whether the preview pause is on; games skip simulation while it is.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Toggle the preview pause.
    pub fn toggle_paused(&mut self) {
        self.paused = !self.paused;
    }

    /// Advance the render clock by a frame's elapsed seconds unless paused, and return it. Time-varying effects (film
    /// grain) read this clock so a paused frame holds still.
    pub fn render_time(&mut self, elapsed_seconds: f64) -> f64 {
        if !self.paused {
            self.clock += elapsed_seconds;
        }
        self.clock
    }

    /// The settings to display at `view_point`: `editor`'s preview while a game has one (development builds), otherwise
    /// the volumes as authored. Games call this once per frame and pass the result to the renderer.
    pub fn resolve(editor: Option<&Self>, volumes: &mut PostProcessVolumes, view_point: Vec3) -> PostProcessSettings {
        match editor {
            Some(editor) => editor.preview(volumes, view_point),
            None => volumes.resolve(view_point),
        }
    }

    /// The settings to display at `view_point`: the volumes resolved with this editor's draft (or, comparing, the
    /// saved settings) in place of the edited volume's, or neutral settings while effects are bypassed.
    pub fn preview(&self, volumes: &mut PostProcessVolumes, view_point: Vec3) -> PostProcessSettings {
        let shown = if self.compare_saved { self.session.saved() } else { self.session.draft() };
        volumes.set_overrides(&self.entity, shown.clone());
        if self.bypassed {
            // Still resolve linear scene color to the display.
            return PostProcessSettings::NEUTRAL;
        }
        volumes.resolve(view_point)
    }

    /// Whether Save can write the level.
    pub fn can_save(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.file.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Draw the launcher and, when open, the panel; then apply the requested edits.
    pub fn ui(&mut self, context: &Context) {
        self.poll_file();
        if self.pause_shortcut {
            dock::claim_key(context, joy_input::key::KEY_P);
            let pressed = context.input(|input| input.key_pressed(egui::Key::P) && input.modifiers.is_none());
            if pressed && !context.egui_wants_keyboard_input() {
                self.toggle_paused();
            }
        }

        let label = if self.session.dirty() { "JOY-ENGINE •" } else { "JOY-ENGINE" };
        dock::launcher(context, DockTool::PostProcessing, RichText::new(label));
        let mut actions = Vec::new();
        dock::panel(context, DockTool::PostProcessing, "POST-PROCESSING", |ui| self.panel(ui, &mut actions));
        for action in actions {
            self.perform(context, action);
        }
    }

    fn panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let session = &self.session;
        ui.label(RichText::new(&self.title).color(dock::MUTED));
        ui.label(RichText::new(format!("Volume · {}", self.entity)).color(dock::MUTED));
        let status = if !self.error.is_empty() {
            RichText::new(&self.error).color(dock::ERROR)
        } else {
            let state = if session.saving() {
                "Saving…"
            } else if session.dirty() {
                "Unsaved Changes"
            } else {
                "Saved"
            };
            let preview = if self.bypassed {
                " · Effects Bypassed"
            } else if self.compare_saved {
                " · Showing Saved"
            } else {
                ""
            };
            let notice = if self.notice.is_empty() { String::new() } else { format!(" · {}", self.notice) };
            RichText::new(format!("{state}{preview}{notice}")).color(dock::TEXT)
        };
        ui.label(status);

        ui.horizontal_wrapped(|ui| {
            let can_save = self.can_save() && session.dirty() && session.conflict().is_none() && !session.saving();
            let save = ui.add_enabled(can_save, egui::Button::new("Save to Level"));
            let save = if self.can_save() {
                save
            } else {
                save.on_disabled_hover_text("Saving needs a native development build with the level's file path.")
            };
            if save.clicked() {
                actions.push(Action::Save);
            }
            if ui.add_enabled(session.dirty(), egui::Button::new("Revert")).clicked() {
                actions.push(Action::Revert);
            }
            if ui.add_enabled(session.can_undo(), egui::Button::new("Undo")).clicked() {
                actions.push(Action::Undo);
            }
            if ui.add_enabled(session.can_redo(), egui::Button::new("Redo")).clicked() {
                actions.push(Action::Redo);
            }
        });

        if session.conflict().is_some() {
            ui.group(|ui| {
                ui.label("The level changed while you had local edits.");
                ui.horizontal(|ui| {
                    if ui.button("Use Disk").clicked() {
                        actions.push(Action::UseDisk);
                    }
                    if ui.button("Keep Local").clicked() {
                        actions.push(Action::KeepLocal);
                    }
                });
            });
        }

        ui.horizontal_wrapped(|ui| {
            let mut paused = self.paused;
            if ui.checkbox(&mut paused, "Pause Simulation").changed() {
                actions.push(Action::Paused(paused));
            }
            let mut compare = self.compare_saved;
            if ui.checkbox(&mut compare, "Compare Saved").changed() {
                actions.push(Action::CompareSaved(compare));
            }
            let mut bypass = self.bypassed;
            if ui.checkbox(&mut bypass, "Bypass Effects").changed() {
                actions.push(Action::Bypass(bypass));
            }
        });
        dock::note(ui, "Bypass keeps a neutral display resolve. Preview modes are never saved.");

        let record = session.settings().to_record();
        let agx = record.get("toneMapper").and_then(Value::as_str) == Some("agx");
        let mut group = "";
        ui.spacing_mut().slider_width = 150.0;
        for control in POST_PROCESS_CONTROLS {
            if control.key == "agxLook" && !agx {
                continue;
            }
            if control.group != group {
                group = control.group;
                dock::heading(ui, group);
            }
            let overridden = session.draft().contains(control.key);
            ui.horizontal(|ui| {
                ui.label(control.label).on_hover_text(control.description);
                let source = if overridden { "Volume Setting" } else { "Inherited" };
                ui.label(RichText::new(source).small().color(dock::MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(overridden, egui::Button::new("Reset").small()).clicked() {
                        actions.push(Action::Reset(control.key));
                    }
                });
            });
            match control.default {
                PostProcessControlDefault::Choice(_) => {
                    let current = record.get(control.key).and_then(Value::as_str).unwrap_or_default().to_owned();
                    let mut selected = current.clone();
                    egui::ComboBox::from_id_salt(("joy-post", control.key))
                        .selected_text(current.to_uppercase())
                        .show_ui(ui, |ui| {
                            for choice in control.choices {
                                ui.selectable_value(&mut selected, (*choice).to_owned(), choice.to_uppercase());
                            }
                        })
                        .response
                        .on_hover_text(control.description);
                    if selected != current {
                        actions.push(Action::Set(control.key, PostProcessValue::Choice(selected)));
                    }
                }
                PostProcessControlDefault::Number(_) => {
                    let current = record.get(control.key).and_then(Value::as_f64).unwrap_or_default();
                    let step = control.step.unwrap_or(0.01);
                    let minimum = match control.minimum {
                        Some(_) if control.exclusive_minimum => step,
                        Some(minimum) => minimum,
                        None => 0.0,
                    };
                    let slider_maximum =
                        control.slider_maximum.or(control.maximum).unwrap_or(minimum + 1.0).max(minimum);
                    let maximum = control.maximum.unwrap_or(f64::MAX);
                    let mut value = current;
                    ui.horizontal(|ui| {
                        let mut slider_value = value.min(slider_maximum);
                        let slider = egui::Slider::new(&mut slider_value, minimum..=slider_maximum)
                            .step_by(step)
                            .show_value(false);
                        if ui.add(slider).on_hover_text(control.description).changed() {
                            value = slider_value;
                        }
                        let decimals = dock::step_decimals(step);
                        ui.add(
                            egui::DragValue::new(&mut value)
                                .speed(step)
                                .range(minimum..=maximum)
                                .fixed_decimals(decimals),
                        );
                    });
                    // Settings are f32, so compare at the control's precision: widgets that round 0.3499999940 to
                    // 0.35 are not edits.
                    let edited = dock::round_to_step(value, step);
                    if edited != dock::round_to_step(current, step) {
                        actions.push(Action::Set(control.key, PostProcessValue::Number(edited)));
                    }
                }
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Export Settings").clicked() {
                actions.push(Action::Export);
            }
            if ui.button(if self.import_open { "Cancel Import" } else { "Import Settings" }).clicked() {
                self.import_open = !self.import_open;
                self.import_text.clear();
            }
        });
        if self.import_open {
            ui.add(
                egui::TextEdit::multiline(&mut self.import_text)
                    .hint_text("Paste a postProcess record's JSON")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            if ui.add_enabled(!self.import_text.trim().is_empty(), egui::Button::new("Preview Import")).clicked() {
                actions.push(Action::Import);
            }
        }
        dock::note(
            ui,
            "Export copies the volume's settings to the clipboard. Import previews a pasted postProcess record's settings; Save to Level commits them.",
        );
    }

    fn perform(&mut self, context: &Context, action: Action) {
        self.notice.clear();
        match action {
            Action::Set(key, value) => self.change(|session| session.set_value(key, value)),
            Action::Reset(key) => self.change(|session| {
                session.reset_value(key);
                Ok(())
            }),
            Action::Undo => self.change(|session| {
                session.undo();
                Ok(())
            }),
            Action::Redo => self.change(|session| {
                session.redo();
                Ok(())
            }),
            Action::Revert => self.change(|session| {
                session.revert();
                Ok(())
            }),
            Action::UseDisk => self.change(|session| {
                session.use_disk();
                Ok(())
            }),
            Action::KeepLocal => self.change(|session| {
                session.keep_local();
                Ok(())
            }),
            Action::Save => self.save(),
            Action::Export => match serde_json::to_string_pretty(self.session.draft()) {
                Ok(text) => {
                    context.copy_text(format!("{text}\n"));
                    self.notice = "Settings copied to the clipboard".to_owned();
                }
                Err(error) => self.error = error.to_string(),
            },
            Action::Import => {
                let text = std::mem::take(&mut self.import_text);
                if text.len() > MAXIMUM_IMPORT_BYTES {
                    self.error = "Settings must be at most 16 KiB.".to_owned();
                    return;
                }
                match serde_json::from_str::<Value>(&text) {
                    Ok(Value::Object(record)) => {
                        // A whole postProcess record imports its settings; its volume fields stay with this volume.
                        let component = PostProcessComponent::read(&record);
                        match component {
                            Ok(component) => {
                                self.change(|session| session.replace(component.overrides.record()));
                                self.import_open = false;
                            }
                            Err(error) => {
                                self.error = error.to_string();
                                self.import_text = text;
                            }
                        }
                    }
                    Ok(_) => {
                        self.error = "Paste a postProcess record (a JSON object).".to_owned();
                        self.import_text = text;
                    }
                    Err(error) => {
                        self.error = format!("Settings are not valid JSON: {error}");
                        self.import_text = text;
                    }
                }
            }
            Action::Paused(paused) => self.paused = paused,
            Action::CompareSaved(compare) => self.compare_saved = compare,
            Action::Bypass(bypass) => self.bypassed = bypass,
        }
    }

    /// Run one session operation; failures leave the session unchanged and show their message.
    fn change(&mut self, operation: impl FnOnce(&mut PostProcessSession) -> Result<(), PostProcessError>) {
        match operation(&mut self.session) {
            Ok(()) => self.error.clear(),
            Err(error) => self.error = error.to_string(),
        }
    }

    /// Pick up outside edits to the level.
    fn poll_file(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(result) = self.file.as_mut().and_then(VolumeFile::poll) else {
                return;
            };
            match result {
                Ok(snapshot) => self.session.receive_external(snapshot.overrides, &snapshot.revision),
                Err(error) => self.error = error,
            }
        }
    }

    fn save(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.session.saving() || self.session.conflict().is_some() {
                return;
            }
            let Some(file) = self.file.as_mut() else {
                return;
            };
            let submitted = self.session.begin_save();
            match file.save(&submitted, self.session.revision()) {
                Ok(revision) => {
                    self.session.mark_saved(submitted, &revision);
                    self.error.clear();
                }
                Err(SaveError::Conflict(snapshot, message)) => {
                    self.session.end_save();
                    self.session.receive_external(snapshot.overrides, &snapshot.revision);
                    self.error = message;
                }
                Err(SaveError::Failed(message)) => {
                    self.session.end_save();
                    self.error = message;
                }
            }
        }
    }
}

/// The volume to edit in a level document: the named entity's, or the first enabled unbounded one.
fn find_volume(level: &Value, entity: Option<&str>) -> Result<(String, PostProcessComponent), PostProcessEditorError> {
    let entities = level.get("entities").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for candidate in entities {
        let Some(id) = candidate.get("id").and_then(Value::as_str) else {
            continue;
        };
        if entity.is_some_and(|wanted| wanted != id) {
            continue;
        }
        let Some(record) = candidate
            .get("components")
            .and_then(|components| components.get(component_type::POST_PROCESS))
            .and_then(Value::as_object)
        else {
            continue;
        };
        let component = PostProcessComponent::read(record).map_err(|error| PostProcessEditorError::InvalidVolume {
            entity: id.to_owned(),
            message: error.to_string(),
        })?;
        if entity.is_some() || (component.enabled && component.unbounded) {
            return Ok((id.to_owned(), component));
        }
    }
    let detail = entity.map_or_else(String::new, |entity| format!(" on entity {entity}"));
    Err(PostProcessEditorError::NoVolume(detail))
}

#[cfg(test)]
#[path = "post_process_editor.test.rs"]
mod tests;
