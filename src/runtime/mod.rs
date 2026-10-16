// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime (platform layer): the game shell that owns windows or browser canvases, the GPU and renderer, input and
//! gamepads, the frame clock, and development UI, and runs a [`Game`]'s phases every frame; plus sound playback and
//! persistent text storage for saves.
//!
//! The same game code runs as a native application (Windows, macOS, Linux) and in the browser through WebAssembly; the
//! shell hides the differences (asynchronous browser startup, canvas sizing, WebGPU or WebGL 2 selection). Immediate-
//! mode UI (egui) draws HUDs and development tools on every target, with [`theme`] colors and [`hud_text`] typography.
//! [`save_text_file`] and [`save_binary_file`] hand generated exports to the player (a browser download or a file in the working directory),
//! and [`Stopwatch`] times CPU work on every target.

pub(crate) mod audio;
mod clock;
pub(crate) mod egui_layer;
mod game;
mod game_loop;
mod gamepads;
#[cfg(not(target_arch = "wasm32"))]
mod headless;
pub mod hud_text;
pub(crate) mod input;
mod launch;
mod save_file;
pub(crate) mod shell;
mod storage;
mod surface;
pub mod theme;

pub use audio::{
    AudioBank, AudioBankOptions, AudioError, DEFAULT_MAXIMUM_VOICES, PcmSound, SoundDefinition, SoundSource,
};
pub use clock::{DEFAULT_MAXIMUM_FRAME_SECONDS, FrameClock, Stopwatch, unix_time_milliseconds};
pub use game::{Frame, FrameTime, Game, GameError, GameResult, RenderTarget, StartContext, Viewport};
pub use game_loop::{DEFAULT_UPDATES_PER_SECOND, FixedStepLoop, MAXIMUM_FRAME_SECONDS};
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{HeadlessOptions, HeadlessShell};
pub use input::{
    ActionState, DEFAULT_DEAD_ZONE, GAMEPLAY_CONTEXT, GamepadState, InputAction, InputAxis, InputBinding, InputError,
    InputManager, InputMap, PRESS_THRESHOLD, PointerState, gamepad, key,
};
pub use launch::launch_parameter;
pub use save_file::{SaveFileError, save_binary_file, save_text_file};
pub use shell::{ShellError, ShellOptions, run};
pub use storage::{MAXIMUM_STORAGE_BYTES, Storage, StorageError};
