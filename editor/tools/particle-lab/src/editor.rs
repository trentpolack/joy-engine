// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One lab script editor (the TypeScript `editor.ts`): the kit's script editor with the particle language catalog,
//! the effect's parameter names highlighted, a status line with the cursor position and a Script Reference button,
//! and a fresh history whenever the owner switches emitters or documents, so programmatic refreshes never become undo
//! entries.

use joy_engine::development::{ScriptDiagnostic, ScriptEditor, ScriptEditorResponse, ScriptSyntax, components};
use joy_engine::egui;
use joy_engine::particles::particle_language_catalog;

/// What the editor did this frame.
pub struct ParticleScriptEditorResponse {
    /// The kit editor's response (changes, Ctrl/Cmd+Enter, cursor).
    pub editor: ScriptEditorResponse,
    /// The Script Reference button was clicked.
    pub reference_requested: bool,
}

/// A particle script editor with its status line.
pub struct ParticleScriptEditor {
    editor: ScriptEditor,
    parameters: Vec<String>,
    owner: Option<(u64, usize)>,
}

impl ParticleScriptEditor {
    /// An editor identified by `id` (unique per editor).
    pub fn new(id: &str) -> Self {
        Self {
            editor: ScriptEditor::new(id, ScriptSyntax::particle()).with_catalog(particle_language_catalog()),
            parameters: Vec::new(),
            owner: None,
        }
    }

    /// The kit editor, for focus and navigation.
    pub fn editor_mut(&mut self) -> &mut ScriptEditor {
        &mut self.editor
    }

    /// Draw the editor for `text`, owned by emitter `emitter` of document `generation`, filling `height` points above
    /// the status line.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        text: &mut String,
        owner: (u64, usize),
        parameters: Vec<String>,
        diagnostics: Vec<ScriptDiagnostic>,
        height: f32,
    ) -> ParticleScriptEditorResponse {
        if self.owner != Some(owner) {
            // A document or emitter switch starts a new history.
            self.owner = Some(owner);
            self.editor.reset_history();
        }
        if parameters != self.parameters {
            self.editor.set_syntax(ScriptSyntax::particle().with_parameters(parameters.iter().map(String::as_str)));
            self.parameters = parameters;
        }
        if self.editor.diagnostics() != diagnostics.as_slice() {
            self.editor.set_diagnostics(diagnostics);
        }
        let editor =
            ui.allocate_ui(egui::vec2(ui.available_width(), height.max(80.0)), |ui| self.editor.show(ui, text)).inner;
        let mut reference_requested = false;
        ui.horizontal(|ui| {
            components::guidance(ui, editor.status_text());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reference_requested = ui.small_button("Script Reference").clicked();
            });
        });
        ParticleScriptEditorResponse { editor, reference_requested }
    }
}
