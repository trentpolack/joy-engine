// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM script editor (UI module): the authoring kit's `ScriptEditor` with FORM highlighting, the engine's FORM
//! language catalog for built-in completion, call insertion, hover documentation, and diagnostics, and the engine's
//! document scan for the variables, parameters, functions, enums, modules, and attributes in scope at the cursor. The
//! kit's `ScriptSyntax::form` supplies highlighting rules.

use joy_engine::development::{
    ScriptEditor,
    ScriptSyntax,
};

/// A FORM script editor for one FORM EDITOR view.
pub fn create_editor() -> ScriptEditor {
    ScriptEditor::new("form-editor-script", ScriptSyntax::form())
        .with_catalog(joy_engine::form::form_language_catalog())
        .with_document_symbols(joy_engine::form::form_document_symbols)
}
