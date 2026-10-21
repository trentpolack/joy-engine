// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine development tools: the development dock games and tools share (the standard CONFIG and JOY-EDITOR
//! menus), the runtime statistics overlay and console ([`DevTools`], [`Console`], and [`ConsoleVariables`]), config
//! panels and [`LiveTuning`], the [`PostProcessEditor`] for post-process volumes, and [`SceneEffectsControls`].
//! [`GameAssets`] embeds a game's text assets and hot reloads them from its project folder in native development builds.
//! Production builds disable the overlay and console through the shell's development option and do not create editors.

#[cfg(not(target_arch = "wasm32"))]
mod config_file;
mod config_panel;
mod config_session;
mod console;
mod console_variables;
mod dev_editors;
mod dev_tools;
pub mod dock;
#[cfg(not(target_arch = "wasm32"))]
mod files;
mod game_assets;
mod live_tuning;
mod post_process_editor;
mod post_process_session;
pub mod render_variables;
mod scene_effects_controls;
#[cfg(not(target_arch = "wasm32"))]
mod volume_file;

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
pub use console::{
    Console,
    ConsoleLine,
    LineKind,
    StatOverlays,
};
pub use console_variables::{
    ConsoleError,
    ConsoleVariables,
    VariableDefinition,
    VariableKind,
    VariableValue,
};
pub use dev_editors::{
    DevEditors,
    DevEditorsOptions,
    TuningPanel,
    development_file,
};
pub use dev_tools::DevTools;
pub use game_assets::{
    ASSET_POLL_SECONDS,
    AssetReadError,
    EmbeddedAsset,
    GameAssets,
};
pub use live_tuning::{
    LiveTuning,
    TuningError,
    TuningField,
};
pub use post_process_editor::{
    PostProcessEditor,
    PostProcessEditorError,
    PostProcessEditorOptions,
};
pub use post_process_session::{
    PostProcessConflict,
    PostProcessSession,
    PostProcessValue,
};
pub use scene_effects_controls::{
    EffectPreset,
    EffectQuality,
    EffectTuning,
    SceneEffectsControls,
};
