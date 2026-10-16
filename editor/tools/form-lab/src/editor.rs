// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM script editor (UI module): the authoring kit's `ScriptEditor` with FORM highlighting and the engine's FORM
//! language catalog for completion, call insertion, hover documentation, and diagnostics. The kit's `ScriptSyntax::form`
//! supplies highlighting rules.

use joy_engine::development::{ScriptEditor, ScriptSyntax};

/// A FORM script editor for one FORM LAB view.
pub fn create_editor() -> ScriptEditor {
    ScriptEditor::new("form-lab-script", ScriptSyntax::form()).with_catalog(joy_engine::form::form_language_catalog())
}
