// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime (platform layer): the shell that owns windows or browser canvases, the GPU and renderer, input and
//! gamepads, the frame clock, and development UI, and runs an [`App`]'s phases every frame; plus sound playback,
//! persistent text storage, and [`SaveSlot`], which keeps a save that could not be read from being overwritten.
//!
//! The same app code runs as a native application (Windows, macOS, Linux) and in the browser through WebAssembly; the
//! shell hides the differences (asynchronous browser startup, canvas sizing, WebGPU or WebGL 2 selection). Immediate-
//! mode UI (egui) draws HUDs and development tools on every target, with [`theme`] colors and [`hud_text`] typography.
//! [`save_text_file`] and [`save_binary_file`] hand generated exports to the player (a browser download or a file in
//! the working directory), [`Stopwatch`] times CPU work on every target, and [`AssetFetcher`] streams asset files
//! without blocking a frame (a worker thread natively, HTTP fetches in browsers).
//!
//! This module gathers the platform crates apps use: the shell, frame phases, and clocks from `joy-app`; input and
//! gamepads from `joy-input`; sound from `joy-audio`; storage, save slots, and exports from `joy-storage`; and the
//! viewport, theme, and HUD typography from `joy-ui`.

pub use joy_app::{
    App,
    AppError,
    AppResult,
    AssetFetcher,
    DEFAULT_MAXIMUM_FRAME_SECONDS,
    Frame,
    FrameClock,
    FrameTime,
    RenderTarget,
    ShellError,
    ShellOptions,
    StartContext,
    Stopwatch,
    launch_parameter,
    run,
    unix_time_milliseconds,
};
#[cfg(not(target_arch = "wasm32"))]
pub use joy_app::{
    CAPTURE_DIRECTORY_VARIABLE,
    HeadlessOptions,
    HeadlessShell,
    region_mean_color,
};
pub use joy_audio::{
    AudioBank,
    AudioBankOptions,
    AudioError,
    DEFAULT_MAXIMUM_VOICES,
    PcmSound,
    SoundDefinition,
    SoundSource,
};
pub use joy_input::{
    ActionState,
    DEFAULT_DEAD_ZONE,
    GAMEPLAY_CONTEXT,
    GamepadState,
    InputAction,
    InputAxis,
    InputBinding,
    InputError,
    InputManager,
    InputMap,
    PRESS_THRESHOLD,
    PointerState,
    gamepad,
    key,
};
pub use joy_storage::{
    MAXIMUM_STORAGE_BYTES,
    Storage,
    StorageError,
};
pub use joy_storage::{
    SaveFileError,
    save_binary_file,
    save_text_file,
};
pub use joy_storage::{
    SaveGuard,
    SaveSlot,
    SaveWrite,
};
pub use joy_ui::{
    Viewport,
    hud_text,
    theme,
};
