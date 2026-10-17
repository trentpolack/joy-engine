// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The in-game post-processing editor: live display tuning with a
//! preset, per-setting overrides, undo, preview modes, and saving back to the game's profile.
//!
//! A game owns the editor in development builds, draws it from its UI phase, and applies
//! [`take_change`](PostProcessingEditor::take_change) to its renderer. Native builds given the profile's document path
//! save into it and pick up outside edits; browser builds have no file access, so Export copies a portable profile to
//! the clipboard instead.

use std::path::PathBuf;

use egui::{
    Context,
    RichText,
};
use serde_json::Value;

use crate::core::post_processing::{
    ControlDefault,
    POST_PROCESSING_SCHEMA,
    PRESET_NAMES,
    ToneMapper,
};
use crate::core::{
    PostProcessingConfig,
    PostProcessingError,
};
use crate::development::dock::{
    self,
    DockTool,
};
use crate::development::post_processing_session::{
    ControlValue,
    PostProcessingSession,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::development::profile_file::{
    ProfileFile,
    SaveError,
};

/// Largest profile accepted by Import.
const MAXIMUM_IMPORT_BYTES: usize = 16 * 1024;

/// Editor configuration.
#[derive(Clone, Debug)]
pub struct PostProcessingEditorOptions {
    /// Game title shown in the panel.
    pub title: String,
    /// The game's authored profile.
    pub profile: Value,
    /// Native development builds: the JSON document whose `postProcessing` key holds the profile. Save writes it and
    /// outside edits reload. Ignored in browsers.
    pub file: Option<PathBuf>,
    /// Whether the P key toggles the preview pause. Games that bind P themselves turn this off.
    pub pause_shortcut: bool,
}

impl Default for PostProcessingEditorOptions {
    fn default() -> Self {
        Self { title: String::new(), profile: Value::Null, file: None, pause_shortcut: true }
    }
}

/// One user request from the panel, applied after drawing.
enum Action {
    Set(&'static str, ControlValue),
    Reset(&'static str),
    Preset(String),
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

/// Live display tuning for one game's renderer.
pub struct PostProcessingEditor {
    title: String,
    session: PostProcessingSession,
    pause_shortcut: bool,
    paused: bool,
    compare_saved: bool,
    bypassed: bool,
    error: String,
    notice: String,
    changed: bool,
    clock: f64,
    import_open: bool,
    import_text: String,
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<ProfileFile>,
}

impl PostProcessingEditor {
    /// Validate the authored profile and, natively, load the profile document.
    pub fn new(options: PostProcessingEditorOptions) -> Result<Self, PostProcessingError> {
        let session = PostProcessingSession::new(&options.profile, "")?;
        let editor = Self {
            title: options.title,
            session,
            pause_shortcut: options.pause_shortcut,
            paused: false,
            compare_saved: false,
            bypassed: false,
            error: String::new(),
            notice: String::new(),
            changed: true,
            clock: 0.0,
            import_open: false,
            import_text: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            file: options.file.map(ProfileFile::new),
        };
        #[cfg(not(target_arch = "wasm32"))]
        let mut editor = editor;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(file) = &mut editor.file {
            match file.read() {
                Ok(snapshot) => {
                    editor.change(|session| session.receive_external(&snapshot.profile, &snapshot.revision))
                }
                Err(error) => {
                    editor.error = format!("Cannot load profile: {error}");
                    editor.file = None;
                }
            }
        }
        Ok(editor)
    }

    /// The editing session, for tests and tools.
    pub fn session(&self) -> &PostProcessingSession {
        &self.session
    }

    /// Whether the preview pause is on; games skip simulation while it is.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Toggle the preview pause.
    pub fn toggle_paused(&mut self) {
        self.paused = !self.paused;
        self.changed = true;
    }

    /// Advance the render clock by a frame's elapsed seconds unless paused, and return it. Time-varying display
    /// effects (film grain) read this clock so a paused frame holds still.
    pub fn render_time(&mut self, elapsed_seconds: f64) -> f64 {
        if !self.paused {
            self.clock += elapsed_seconds;
        }
        self.clock
    }

    /// The settings the renderer should display now, including Compare Saved and Bypass Effects previews.
    pub fn config(&self) -> PostProcessingConfig {
        if self.bypassed {
            // Still resolve linear scene color to the display.
            return PostProcessingConfig {
                tone_mapper: ToneMapper::None,
                antialias_strength: 0.0,
                ..PostProcessingConfig::default()
            };
        }
        if self.compare_saved { self.session.saved_config() } else { self.session.config() }
    }

    /// The settings to restore when the editor goes away: the saved profile, never a preview.
    pub fn saved_config(&self) -> PostProcessingConfig {
        self.session.saved_config()
    }

    /// The display settings when they changed since the last call; apply them to the renderer.
    pub fn take_change(&mut self) -> Option<PostProcessingConfig> {
        std::mem::take(&mut self.changed).then(|| self.config())
    }

    /// Whether Save can write the profile.
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
            dock::claim_key(context, crate::runtime::key::KEY_P);
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
            let save = ui.add_enabled(can_save, egui::Button::new("Save to Game"));
            let save = if self.can_save() {
                save
            } else {
                save.on_disabled_hover_text("Saving needs a native development build with the profile's file path.")
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
                ui.label("The file changed while you had local edits.");
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

        ui.horizontal(|ui| {
            ui.label("Starting Preset");
            let mut preset = session.draft().preset.clone();
            egui::ComboBox::from_id_salt("joy-post-preset").selected_text(title_case(&preset)).show_ui(ui, |ui| {
                for name in PRESET_NAMES {
                    ui.selectable_value(&mut preset, (*name).to_owned(), title_case(name));
                }
            });
            if preset != session.draft().preset {
                actions.push(Action::Preset(preset));
            }
        });
        dock::note(ui, "Choosing a preset clears overrides. Undo restores your previous look.");

        let record = session.config().to_record();
        let agx = record.get("toneMapper").and_then(Value::as_str) == Some("agx");
        let mut group = "";
        ui.spacing_mut().slider_width = 150.0;
        for control in POST_PROCESSING_SCHEMA {
            if control.key == "agxLook" && !agx {
                continue;
            }
            if control.group != group {
                group = control.group;
                dock::heading(ui, group);
            }
            let overridden = session.draft().overrides.contains_key(control.key);
            ui.horizontal(|ui| {
                ui.label(control.label).on_hover_text(control.description);
                let source = if overridden {
                    "Game Override".to_owned()
                } else {
                    format!("Inherited · {}", session.draft().preset)
                };
                ui.label(RichText::new(source).small().color(dock::MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(overridden, egui::Button::new("Reset").small()).clicked() {
                        actions.push(Action::Reset(control.key));
                    }
                });
            });
            match control.default {
                ControlDefault::Choice(_) => {
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
                        actions.push(Action::Set(control.key, ControlValue::Choice(selected)));
                    }
                }
                ControlDefault::Number(_) => {
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
                        actions.push(Action::Set(control.key, ControlValue::Number(edited)));
                    }
                }
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Export Preset").clicked() {
                actions.push(Action::Export);
            }
            if ui.button(if self.import_open { "Cancel Import" } else { "Import Profile" }).clicked() {
                self.import_open = !self.import_open;
                self.import_text.clear();
            }
        });
        if self.import_open {
            ui.add(
                egui::TextEdit::multiline(&mut self.import_text)
                    .hint_text("Paste a profile's JSON")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            if ui.add_enabled(!self.import_text.trim().is_empty(), egui::Button::new("Preview Import")).clicked() {
                actions.push(Action::Import);
            }
        }
        dock::note(
            ui,
            "Export copies a portable snapshot to the clipboard. Import previews a pasted profile; Save to Game commits it.",
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
            Action::Preset(preset) => self.change(|session| session.select_preset(&preset)),
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
            Action::Export => {
                // Flatten inheritance for a portable look that survives later preset edits.
                let profile = serde_json::json!({
                    "version": 1,
                    "preset": "default",
                    "overrides": self.session.config().to_record(),
                });
                match serde_json::to_string_pretty(&profile) {
                    Ok(text) => {
                        context.copy_text(format!("{text}\n"));
                        self.notice = "Profile copied to the clipboard".to_owned();
                    }
                    Err(error) => self.error = error.to_string(),
                }
            }
            Action::Import => {
                let text = std::mem::take(&mut self.import_text);
                if text.len() > MAXIMUM_IMPORT_BYTES {
                    self.error = "Profile must be at most 16 KiB.".to_owned();
                    return;
                }
                match serde_json::from_str::<Value>(&text) {
                    Ok(profile) => {
                        self.change(|session| session.replace(&profile));
                        if self.error.is_empty() {
                            self.import_open = false;
                        } else {
                            self.import_text = text;
                        }
                    }
                    Err(error) => {
                        self.error = format!("Profile is not valid JSON: {error}");
                        self.import_text = text;
                    }
                }
            }
            Action::Paused(paused) => {
                self.paused = paused;
                self.changed = true;
            }
            Action::CompareSaved(compare) => {
                self.compare_saved = compare;
                self.changed = true;
            }
            Action::Bypass(bypass) => {
                self.bypassed = bypass;
                self.changed = true;
            }
        }
    }

    /// Run one session operation; failures leave the session unchanged and show their message.
    fn change(&mut self, operation: impl FnOnce(&mut PostProcessingSession) -> Result<(), PostProcessingError>) {
        match operation(&mut self.session) {
            Ok(()) => {
                self.error.clear();
                self.changed = true;
            }
            Err(error) => self.error = error.to_string(),
        }
    }

    /// Pick up outside edits to the profile document.
    fn poll_file(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(result) = self.file.as_mut().and_then(ProfileFile::poll) else {
                return;
            };
            match result {
                Ok(snapshot) => self.change(|session| session.receive_external(&snapshot.profile, &snapshot.revision)),
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
                    self.changed = true;
                }
                Err(SaveError::Conflict(snapshot, message)) => {
                    self.session.end_save();
                    self.change(|session| session.receive_external(&snapshot.profile, &snapshot.revision));
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

/// `soft` → `Soft`.
fn title_case(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| first.to_uppercase().chain(characters).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn editor() -> PostProcessingEditor {
        let profile = json!({ "version": 1, "preset": "default", "overrides": { "exposure": 2 } });
        PostProcessingEditor::new(PostProcessingEditorOptions { title: "Test".into(), profile, ..Default::default() })
            .unwrap()
    }

    #[test]
    fn previews_change_the_display_but_never_the_draft() {
        let mut editor = editor();
        assert_eq!(editor.take_change().map(|config| config.exposure), Some(2.0));
        assert!(editor.take_change().is_none());
        let context = Context::default();
        editor.perform(&context, Action::Set("exposure", ControlValue::Number(3.0)));
        assert_eq!(editor.take_change().map(|config| config.exposure), Some(3.0));
        editor.perform(&context, Action::CompareSaved(true));
        assert_eq!(editor.take_change().map(|config| config.exposure), Some(2.0));
        editor.perform(&context, Action::Bypass(true));
        let bypassed = editor.take_change().unwrap();
        assert_eq!(
            (bypassed.tone_mapper, bypassed.antialias_strength, bypassed.exposure),
            (ToneMapper::None, 0.0, 1.0)
        );
        assert_eq!(editor.session().config().exposure, 3.0);
        assert_eq!(editor.saved_config().exposure, 2.0);
        assert!(!editor.can_save());
    }

    #[test]
    fn invalid_imports_and_edits_report_errors_without_changing_the_draft() {
        let mut editor = editor();
        let context = Context::default();
        editor.import_text = "{".to_owned();
        editor.perform(&context, Action::Import);
        assert!(editor.error.contains("not valid JSON"));
        editor.import_text = json!({ "version": 2 }).to_string();
        editor.perform(&context, Action::Import);
        assert!(!editor.error.is_empty());
        editor.perform(&context, Action::Set("exposure", ControlValue::Number(-1.0)));
        assert!(!editor.error.is_empty());
        assert_eq!(editor.session().config().exposure, 2.0);
        editor.import_text = json!({ "version": 1, "preset": "vivid", "overrides": {} }).to_string();
        editor.perform(&context, Action::Import);
        assert!(editor.error.is_empty());
        assert_eq!(editor.session().draft().preset, "vivid");
    }

    #[test]
    fn drawing_the_panel_does_not_edit_the_draft() {
        let profile = json!({ "version": 1, "preset": "default", "overrides": { "toneMapper": "agx" } });
        let mut editor =
            PostProcessingEditor::new(PostProcessingEditorOptions { profile, ..Default::default() }).unwrap();
        let context = Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            ..Default::default()
        };
        dock::set_open(&context, DockTool::PostProcessing, true);
        for _ in 0..3 {
            let mut output = context.run_ui(input(), |ui| editor.ui(ui.ctx()));
            output.textures_delta.clear();
        }
        assert_eq!(dock::open_tool(&context), Some(DockTool::PostProcessing));
        assert!(!editor.session().dirty() && !editor.session().can_undo(), "{:?}", editor.session().draft());
    }

    #[test]
    fn the_render_clock_holds_while_paused() {
        let mut editor = editor();
        assert_eq!(editor.render_time(0.5), 0.5);
        editor.toggle_paused();
        assert_eq!(editor.render_time(0.5), 0.5);
        assert!(editor.paused());
    }

    #[test]
    fn preset_names_read_as_titles() {
        assert_eq!(title_case("vivid"), "Vivid");
    }
}
