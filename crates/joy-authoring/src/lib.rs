// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The authoring kit shared by FORM LAB, PARTICLE LAB, and Joy Editor: lab controls ([`NumberField`],
//! [`ScalarControl`], [`ComponentGroup`], [`ColorControl`]) with [`NumericDrafts`], [`PropertyHistory`],
//! [`PanelLayout`], the [`ScriptEditor`] with its [`script_language_tools`], viewport gestures, [`FileDialogs`], and the
//! shared Joy look in [`components`] (see `docs/rust-authoring.md`).
//!
//! [`AuthoringTool`] is the contract that lets authoring tools run standalone (as a `joy-app` [`App`](joy_app::App))
//! and as Joy Editor tabs. Tools reach the kit through `joy_engine::development`, beside the development tools from
//! `joy-devtools`.

mod authoring_tool;
pub mod components;
pub mod file_dialogs;
mod lab_controls;
pub mod lab_values;
pub mod panel_layout;
mod property_history;
mod script_editor;
pub mod script_language_tools;
mod viewport_input;
#[cfg(target_arch = "wasm32")]
mod web_file_input;

pub use authoring_tool::{
    AuthoringTool,
    ToolDocument,
    ToolHosting,
};
pub use file_dialogs::{
    FileContents,
    FileDialogError,
    FileDialogEvent,
    FileDialogs,
    FileFilter,
    OpenRequest,
    OpenedFile,
    SaveRequest,
};
pub use lab_controls::{
    COLOR_SWATCH_SIZE,
    ColorControl,
    ComponentGroup,
    ControlResponse,
    EditPhase,
    NUMBER_FIELD_WIDTH,
    NumberField,
    NumericDrafts,
    RejectedDraft,
    SCALAR_LABEL_WIDTH,
    ScalarControl,
    ValueCheck,
    is_modified,
    lab_slider,
    modified_marker,
    paint_checkerboard,
    reset_button,
};
pub use lab_values::{
    NumericRules,
    StepScale,
};
pub use panel_layout::{
    MaximizeMode,
    PaneLimits,
    PaneSide,
    PanelLayout,
    PanelLayoutOptions,
    PanelRects,
};
pub use property_history::{
    PROPERTY_HISTORY_LIMIT,
    PropertyHistory,
};
pub use script_editor::{
    ScriptEditor,
    ScriptEditorResponse,
    token_color,
};
pub use script_language_tools::{
    CompletionItem,
    DiagnosticSeverity,
    ScriptDiagnostic,
    ScriptSyntax,
    Token,
    TokenKind,
};
pub use viewport_input::{
    ViewportActions,
    ViewportGesture,
    ViewportShortcut,
    viewport_input,
};
