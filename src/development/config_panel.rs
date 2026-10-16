// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The TUNE panel for a [`ConfigSession`] and [`ConfigEditor`], which pairs a session with a game's override document.
//!
//! Games own the editor in development builds, draw it from their UI phase, and apply
//! [`take_change`](ConfigEditor::take_change). Native builds given the document path read it at startup and save into
//! it; browser builds tune for the session only.

use std::path::PathBuf;

use egui::{Context, RichText};
use indexmap::IndexMap;

use crate::core::{ConfigApplyMode, ConfigError, ConfigValues};
#[cfg(not(target_arch = "wasm32"))]
use crate::development::config_file::ConfigFile;
use crate::development::config_session::ConfigSession;
use crate::development::dock::{self, DockTool};

/// One user request from the panel, applied after drawing.
enum Action {
    BeginEdit,
    EndEdit,
    Set(String, f64),
    ResetField(String),
    Undo,
    Redo,
    Revert,
    ResetAll,
    Save,
    Restart,
}

/// What a frame of the panel asked the game to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigPanelResponse {
    /// The restart button was pressed.
    pub restart: bool,
}

/// The TUNE launcher and panel for one session.
#[derive(Debug)]
pub struct ConfigPanel {
    title: String,
    restart_label: Option<String>,
    search: String,
    status: String,
    status_error: bool,
    was_open: bool,
}

impl ConfigPanel {
    /// A panel titled for the game; `restart_label` adds a restart button (for `restart` apply-mode fields).
    pub fn new(title: impl Into<String>, restart_label: Option<String>) -> Self {
        Self {
            title: title.into(),
            restart_label,
            search: String::new(),
            status: String::new(),
            status_error: false,
            was_open: false,
        }
    }

    /// Show a status message, for example a load failure.
    pub fn set_status(&mut self, message: impl Into<String>, error: bool) {
        self.status = message.into();
        self.status_error = error;
    }

    /// Draw the launcher and, when open, the panel; then apply the requested edits to `session`.
    pub fn ui(&mut self, context: &Context, session: &mut ConfigSession) -> ConfigPanelResponse {
        let open = dock::launcher(context, DockTool::Config, RichText::new("TUNE"));
        if self.was_open && !open {
            session.end_edit();
        }
        self.was_open = open;

        let mut actions = Vec::new();
        if open && !context.egui_wants_keyboard_input() {
            context.input(|input| {
                if input.modifiers.command && input.key_pressed(egui::Key::Z) {
                    actions.push(if input.modifiers.shift { Action::Redo } else { Action::Undo });
                }
            });
        }
        let title = self.title.clone();
        dock::panel(context, DockTool::Config, &title, |ui| self.panel(ui, session, &mut actions));

        let mut response = ConfigPanelResponse::default();
        for action in actions {
            let result = match action {
                Action::BeginEdit => {
                    session.begin_edit();
                    Ok(())
                }
                Action::EndEdit => {
                    session.end_edit();
                    Ok(())
                }
                Action::Set(key, value) => session.set_value(&key, value),
                Action::ResetField(key) => session.reset_field(&key),
                Action::Undo => {
                    session.undo();
                    Ok(())
                }
                Action::Redo => {
                    session.redo();
                    Ok(())
                }
                Action::Revert => session.revert(),
                Action::ResetAll => session.reset_all(),
                Action::Save => {
                    session.end_edit();
                    session.save().map(|()| {
                        let message =
                            if session.dirty() { "Saved snapshot; newer edits remain unsaved." } else { "Saved." };
                        self.set_status(message, false);
                    })
                }
                Action::Restart => {
                    response.restart = true;
                    self.set_status("Scenario restarted.", false);
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.set_status(error.to_string(), true);
            }
        }
        response
    }

    fn panel(&mut self, ui: &mut egui::Ui, session: &ConfigSession, actions: &mut Vec<Action>) {
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search settings").desired_width(f32::INFINITY));
        let query = self.search.trim().to_lowercase();

        // Groups appear in the order their first field does.
        let mut groups: IndexMap<&str, Vec<usize>> = IndexMap::new();
        for (index, field) in session.fields().iter().enumerate() {
            groups.entry(field.group.as_str()).or_default().push(index);
        }
        ui.spacing_mut().slider_width = 150.0;
        for (group, indices) in groups {
            let group_matches = group.to_lowercase().contains(&query);
            let visible: Vec<usize> = indices
                .into_iter()
                .filter(|index| {
                    let field = &session.fields()[*index];
                    query.is_empty()
                        || group_matches
                        || [&field.label, &field.description, &field.units, &field.key]
                            .iter()
                            .any(|text| text.to_lowercase().contains(&query))
                })
                .collect();
            if visible.is_empty() {
                continue;
            }
            dock::heading(ui, group);
            for index in visible {
                self.field_row(ui, session, index, actions);
            }
        }

        ui.add_space(8.0);
        let state = if !session.can_save() {
            "SESSION ONLY — changes are not persisted"
        } else if session.dirty() {
            "Unsaved changes"
        } else {
            "No unsaved edits"
        };
        ui.label(RichText::new(state).color(if session.dirty() { dock::ACCENT } else { dock::MUTED }));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(session.can_undo(), egui::Button::new("Undo")).clicked() {
                actions.push(Action::Undo);
            }
            if ui.add_enabled(session.can_redo(), egui::Button::new("Redo")).clicked() {
                actions.push(Action::Redo);
            }
            if ui.add_enabled(session.dirty(), egui::Button::new("Revert")).clicked() {
                actions.push(Action::Revert);
            }
            if ui.button("Reset All").clicked() {
                actions.push(Action::ResetAll);
            }
            if let Some(label) = &self.restart_label
                && ui.button(label).clicked()
            {
                actions.push(Action::Restart);
            }
            if session.can_save() && ui.add_enabled(session.dirty(), egui::Button::new("Save")).clicked() {
                actions.push(Action::Save);
            }
        });
        if !self.status.is_empty() {
            ui.label(RichText::new(&self.status).color(if self.status_error { dock::ERROR } else { dock::TEXT }));
        }
    }

    fn field_row(&self, ui: &mut egui::Ui, session: &ConfigSession, index: usize, actions: &mut Vec<Action>) {
        let field = &session.fields()[index];
        let value = session.value(&field.key).unwrap_or(field.default_value);
        let stored = session.stored_values().get(&field.key).copied() == Some(value);
        ui.horizontal(|ui| {
            ui.label(&field.label).on_hover_text(&field.description);
            ui.label(RichText::new(apply_mode_label(field.apply_mode)).small().color(dock::MUTED));
            let (marker, color) = if !stored {
                ("Unsaved", dock::ACCENT)
            } else if value == field.default_value {
                ("Default", dock::MUTED)
            } else {
                ("Stored override", dock::TEXT)
            };
            ui.label(RichText::new(marker).small().color(color));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let reset = egui::Button::new("Reset").small();
                if ui
                    .add_enabled(value != field.default_value, reset)
                    .on_hover_text(format!("Reset {}", field.label))
                    .clicked()
                {
                    actions.push(Action::ResetField(field.key.clone()));
                }
            });
        });
        dock::note(ui, &field.description);
        ui.horizontal(|ui| {
            let mut slider_value = value;
            let slider = ui
                .add(egui::Slider::new(&mut slider_value, field.min..=field.max).step_by(field.step).show_value(false));
            let mut typed = value;
            let decimals = dock::step_decimals(field.step);
            let drag = ui.add(
                egui::DragValue::new(&mut typed)
                    .speed(field.step)
                    .range(field.min..=field.max)
                    .fixed_decimals(decimals),
            );
            if !field.units.is_empty() {
                ui.label(RichText::new(&field.units).color(dock::MUTED));
            }
            for response in [&slider, &drag] {
                if response.drag_started() {
                    actions.push(Action::BeginEdit);
                }
            }
            let edited = if slider.changed() { slider_value } else { typed };
            let edited = dock::round_to_step(edited, field.step);
            if (slider.changed() || drag.changed()) && edited != dock::round_to_step(value, field.step) {
                actions.push(Action::Set(field.key.clone(), edited));
            }
            if slider.drag_stopped() || drag.drag_stopped() {
                actions.push(Action::EndEdit);
            }
        });
    }
}

/// Display name of an apply mode.
fn apply_mode_label(mode: ConfigApplyMode) -> &'static str {
    match mode {
        ConfigApplyMode::Live => "Live",
        ConfigApplyMode::NextSpawn => "Next Spawn",
        ConfigApplyMode::Restart => "On Restart",
    }
}

/// Config editor configuration.
#[derive(Clone, Debug)]
pub struct ConfigEditorOptions {
    /// Game title shown in the panel.
    pub title: String,
    /// The game's tuning fields.
    pub fields: Vec<crate::core::ConfigField>,
    /// Authored overrides, for example the shipped `tuning-overrides.json` values.
    pub values: IndexMap<String, f64>,
    /// Native development builds: the override document Save writes, read at startup in place of `values`. Ignored
    /// in browsers.
    pub file: Option<PathBuf>,
    /// Label for a restart button, for games with `restart` apply-mode fields.
    pub restart_label: Option<String>,
}

/// A config session, its TUNE panel, and (natively) its override document.
pub struct ConfigEditor {
    session: ConfigSession,
    panel: ConfigPanel,
    restart_requested: bool,
}

impl ConfigEditor {
    /// Validate the fields and authored values and, natively, load the override document. A document that cannot be
    /// read leaves a session-only editor that says why.
    pub fn new(options: ConfigEditorOptions) -> Result<Self, ConfigError> {
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut panel = ConfigPanel::new(options.title, options.restart_label);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = options.file.clone() {
            match ConfigFile::open(path, &options.fields) {
                Ok((mut file, values)) => {
                    let mut session = ConfigSession::new(&options.fields, &values)?
                        .with_save(Box::new(move |values: &ConfigValues| file.save(values)));
                    // The document is the development source of truth; games apply it on the first frame.
                    session.mark_changed();
                    return Ok(Self { session, panel, restart_requested: false });
                }
                Err(error) => panel.set_status(format!("Cannot load config: {error}"), true),
            }
        }
        let session = ConfigSession::new(&options.fields, &options.values)?;
        Ok(Self { session, panel, restart_requested: false })
    }

    /// The editing session.
    pub fn session(&self) -> &ConfigSession {
        &self.session
    }

    /// The editing session, for presets and programmatic edits.
    pub fn session_mut(&mut self) -> &mut ConfigSession {
        &mut self.session
    }

    /// Current values.
    pub fn values(&self) -> &ConfigValues {
        self.session.values()
    }

    /// The values when they changed since the last call; apply them to the game.
    pub fn take_change(&mut self) -> Option<ConfigValues> {
        self.session.take_change()
    }

    /// Whether the restart button was pressed since the last call.
    pub fn take_restart(&mut self) -> bool {
        std::mem::take(&mut self.restart_requested)
    }

    /// Draw the launcher and panel and apply edits.
    pub fn ui(&mut self, context: &Context) {
        if self.panel.ui(context, &mut self.session).restart {
            self.restart_requested = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::development::config_session::tests::fields;

    fn run(context: &Context, mut ui: impl FnMut(&Context)) {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                ..Default::default()
            },
            |root| ui(root.ctx()),
        );
        output.textures_delta.clear();
    }

    #[test]
    fn drawing_the_panel_does_not_edit_and_search_filters_without_errors() {
        let options = ConfigEditorOptions {
            title: "Test".into(),
            fields: fields(),
            values: [("speed".to_owned(), 5.5)].into_iter().collect(),
            file: None,
            restart_label: Some("Restart Scenario".into()),
        };
        let mut editor = ConfigEditor::new(options).unwrap();
        let context = Context::default();
        dock::set_open(&context, DockTool::Config, true);
        for search in ["", "spawn", "nothing matches"] {
            editor.panel.search = search.to_owned();
            run(&context, |context| editor.ui(context));
        }
        assert!(!editor.session().dirty() && !editor.session().can_undo());
        assert_eq!(editor.take_change(), None);
        assert!(!editor.take_restart());
        assert_eq!(editor.values()["speed"], 5.5);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_editors_load_their_document_and_report_it_once() {
        let directory = std::env::temp_dir().join(format!("joy-config-editor-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("tuning-overrides.json");
        std::fs::write(&path, "{\"version\": 1, \"values\": {\"count\": 6}}").unwrap();
        let options = ConfigEditorOptions {
            title: "Test".into(),
            fields: fields(),
            values: IndexMap::new(),
            file: Some(path),
            restart_label: None,
        };
        let mut editor = ConfigEditor::new(options).unwrap();
        assert!(editor.session().can_save());
        assert_eq!(editor.take_change().map(|values| values["count"]), Some(6.0));
        assert_eq!(editor.take_change(), None);
        editor.session_mut().set_value("speed", 6.0).unwrap();
        editor.session_mut().save().unwrap();
        assert!(!editor.session().dirty());
        std::fs::remove_dir_all(directory).unwrap();

        let missing = ConfigEditorOptions {
            title: "Test".into(),
            fields: fields(),
            values: IndexMap::new(),
            file: Some(std::env::temp_dir().join("joy-missing-config.json")),
            restart_label: None,
        };
        let editor = ConfigEditor::new(missing).unwrap();
        assert!(!editor.session().can_save());
        assert!(editor.panel.status.contains("Cannot load config"));
    }
}
