// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Development tooling (platform layer): the development dock and its tools: the runtime statistics overlay, the
//! post-processing editor, scene effect preview controls, config panels, and live tuning. Production builds disable
//! the overlay through `ShellOptions::development` and do not create editors. [`AuthoringTool`] is the contract that
//! lets authoring tools such as FORM LAB run standalone and as Joy Editor tabs.
//!
//! The authoring kit shared by FORM LAB, PARTICLE LAB, and Joy Editor lives here too: lab controls ([`NumberField`],
//! [`ScalarControl`], [`ComponentGroup`], [`ColorControl`]) with [`NumericDrafts`], [`PropertyHistory`],
//! [`PanelLayout`], the [`ScriptEditor`], and the shared Joy look in [`components`] (see `docs/rust-authoring.md`).

mod authoring_tool;
pub mod components;
#[cfg(not(target_arch = "wasm32"))]
mod config_file;
mod config_panel;
mod config_session;
mod dev_tools;
pub mod dock;
pub mod file_dialogs;
#[cfg(not(target_arch = "wasm32"))]
mod files;
mod lab_controls;
pub mod lab_values;
mod live_tuning;
pub mod panel_layout;
mod post_processing_editor;
mod post_processing_session;
#[cfg(not(target_arch = "wasm32"))]
mod profile_file;
mod property_history;
mod scene_effects_controls;
mod script_editor;
pub mod script_language_tools;
mod viewport_input;

pub use authoring_tool::{
    AuthoringTool,
    ToolDocument,
    ToolHosting,
};
pub use config_panel::{
    ConfigEditor,
    ConfigEditorOptions,
    ConfigPanel,
    ConfigPanelResponse,
};
pub use config_session::{
    ConfigChangeAdapter,
    ConfigSaveAdapter,
    ConfigSession,
    ConfigSessionError,
};
pub use dev_tools::DevTools;
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
pub use live_tuning::{
    LiveTuning,
    TuningError,
    TuningField,
};
pub use panel_layout::{
    MaximizeMode,
    PaneLimits,
    PaneSide,
    PanelLayout,
    PanelLayoutOptions,
    PanelRects,
};
pub use post_processing_editor::{
    PostProcessingEditor,
    PostProcessingEditorOptions,
};
pub use post_processing_session::{
    ControlValue,
    PostProcessingSession,
    ProfileConflict,
};
pub use property_history::{
    PROPERTY_HISTORY_LIMIT,
    PropertyHistory,
};
pub use scene_effects_controls::{
    EffectPreset,
    EffectQuality,
    EffectTuning,
    SceneEffectsControls,
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
