// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine development tools: the development dock games and tools share (the standard CONFIG and JOY-EDITOR
//! menus), the runtime statistics overlay ([`DevTools`]), config panels and [`LiveTuning`], the
//! [`PostProcessingEditor`], and [`SceneEffectsControls`]. Production builds disable the overlay through the shell's
//! development option and do not create editors.

#[cfg(not(target_arch = "wasm32"))]
mod config_file;
mod config_panel;
mod config_session;
mod dev_tools;
pub mod dock;
#[cfg(not(target_arch = "wasm32"))]
mod files;
mod live_tuning;
mod post_processing_editor;
mod post_processing_session;
#[cfg(not(target_arch = "wasm32"))]
mod profile_file;
mod scene_effects_controls;

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
pub use live_tuning::{
    LiveTuning,
    TuningError,
    TuningField,
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
pub use scene_effects_controls::{
    EffectPreset,
    EffectQuality,
    EffectTuning,
    SceneEffectsControls,
};
