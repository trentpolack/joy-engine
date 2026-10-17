// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Save and Export dialog (UI module): an explicit format, a file name, and
//! where the file goes. Contents are produced only when the author confirms; the platform's save dialog (natively) or
//! a download (browsers) then writes them through the authoring kit's `FileDialogs`. Cancelling the save dialog keeps
//! this dialog open and writes nothing, and a failed write keeps it open with the error.

use joy_engine::development::components;
use joy_engine::egui::{
    self,
    RichText,
};
use joy_engine::runtime::theme::tokens;

use crate::session::{
    ArtifactFormat,
    EXPORT_FORMATS,
    SAVE_FORMATS,
};

/// Which dialog is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    /// Save editable source (`.form`) or the document (`.formlab`).
    Save,
    /// Export the last accepted build.
    Export,
}

impl ArtifactKind {
    /// The dialog title and confirm button.
    pub fn title(self) -> &'static str {
        match self {
            Self::Save => "Save",
            Self::Export => "Export",
        }
    }

    /// The formats offered.
    pub fn formats(self) -> &'static [ArtifactFormat] {
        match self {
            Self::Save => &SAVE_FORMATS,
            Self::Export => &EXPORT_FORMATS,
        }
    }
}

/// The author confirmed a write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRequest {
    /// Save or Export.
    pub kind: ArtifactKind,
    /// The chosen format.
    pub format: ArtifactFormat,
    /// The file name, with the format's extension.
    pub name: String,
}

/// One Save/Export dialog.
#[derive(Clone, Debug, Default)]
pub struct ArtifactDialog {
    kind: Option<ArtifactKind>,
    format: usize,
    name: String,
    error: Option<String>,
    writing: bool,
}

impl ArtifactDialog {
    /// Open with the first format selected and `stem` as the file name.
    pub fn open(&mut self, kind: ArtifactKind, stem: &str) {
        if self.kind.is_some() {
            return;
        }
        self.kind = Some(kind);
        self.format = 0;
        self.name = format!("{stem}.{}", kind.formats()[0].id);
        self.error = None;
        self.writing = false;
    }

    /// Whether the dialog is open.
    pub fn is_open(&self) -> bool {
        self.kind.is_some()
    }

    /// The open dialog's kind.
    pub fn kind(&self) -> Option<ArtifactKind> {
        self.kind
    }

    /// Choose a format by its extension (tests and keyboard flows).
    pub fn select_format(&mut self, id: &str) {
        if let Some(kind) = self.kind
            && let Some(index) = kind.formats().iter().position(|format| format.id == id)
        {
            self.format = index;
            self.name = with_extension(&self.name, kind.formats()[index].id);
        }
    }

    /// The write finished; close.
    pub fn completed(&mut self) {
        self.kind = None;
        self.writing = false;
    }

    /// The platform dialog was cancelled; stay open without an error.
    pub fn cancelled(&mut self) {
        self.writing = false;
    }

    /// Producing or writing failed; stay open with the message.
    pub fn failed(&mut self, message: impl Into<String>) {
        self.writing = false;
        self.error = Some(message.into());
    }

    /// Validate the name and return the request to write, or show why not.
    pub fn confirm(&mut self) -> Option<ArtifactRequest> {
        let kind = self.kind?;
        if self.writing {
            return None;
        }
        let format = kind.formats()[self.format];
        let mut name = self.name.trim().to_owned();
        if name.is_empty() || name.contains(['/', '\\', '\0']) {
            self.error = Some("Enter a file name without folder separators.".into());
            return None;
        }
        if !name.to_lowercase().ends_with(&format!(".{}", format.id)) {
            name.push('.');
            name.push_str(format.id);
        }
        self.error = None;
        self.writing = true;
        Some(ArtifactRequest { kind, format, name })
    }

    /// Draw the dialog when open; returns a confirmed request. `export_note` describes the build exports use.
    pub fn show(&mut self, context: &egui::Context, export_note: &str) -> Option<ArtifactRequest> {
        let kind = self.kind?;
        let mut request = None;
        let mut close = false;
        egui::Modal::new(egui::Id::new("form-lab-artifact-dialog")).show(context, |ui| {
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new(kind.title()).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!self.writing, egui::Button::new("✕")).on_hover_text("Close file dialog").clicked() {
                        close = true;
                    }
                });
            });
            components::field_title(ui, "Format");
            let formats = kind.formats();
            let mut format = self.format;
            egui::ComboBox::from_id_salt("artifact-format")
                .selected_text(formats[format].label)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (index, candidate) in formats.iter().enumerate() {
                        ui.selectable_value(&mut format, index, candidate.label);
                    }
                });
            if format != self.format {
                self.format = format;
                self.name = with_extension(&self.name, formats[format].id);
            }
            components::guidance(ui, formats[self.format].description);
            ui.add_space(tokens::SPACE_SM);
            components::field_title(ui, "File name");
            let field = ui.add(egui::TextEdit::singleline(&mut self.name).char_limit(160).desired_width(f32::INFINITY));
            let submitted = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            components::guidance(
                ui,
                if cfg!(target_arch = "wasm32") {
                    "Your browser controls the download folder. In Safari, choose “Ask for each download” in Settings → \
                     General → File download location to pick a folder each time."
                } else {
                    "Choose the folder and file name in the save dialog."
                },
            );
            if kind == ArtifactKind::Export {
                components::guidance(ui, export_note);
            }
            if let Some(error) = &self.error {
                components::error_text(ui, error.clone());
            }
            ui.add_space(tokens::SPACE_SM);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let confirm = ui.add_enabled(!self.writing, components::primary_button(kind.title())).clicked();
                if ui.add_enabled(!self.writing, egui::Button::new("Cancel")).clicked() {
                    close = true;
                }
                if confirm || submitted {
                    request = self.confirm();
                }
            });
        });
        if close {
            self.kind = None;
        }
        request
    }
}

/// Replace a known artifact extension (or append one) for a newly chosen format (`updateFormat`).
fn with_extension(name: &str, extension: &str) -> String {
    let lower = name.to_lowercase();
    let stem = [".formlab", ".form", ".glb", ".obj", ".ply", ".json"]
        .iter()
        .find(|known| lower.ends_with(*known))
        .map_or(name, |known| &name[..name.len() - known.len()]);
    format!("{stem}.{extension}")
}
