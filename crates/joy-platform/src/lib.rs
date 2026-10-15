// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Platform layer of Joy Engine: the game shell that owns windows or browser canvases, the GPU and renderer, input,
//! the frame clock, and development UI, and runs a [`Game`]'s phases every frame.
//!
//! The same game code runs as a native application (Windows, macOS, Linux) and in the browser through WebAssembly; the
//! shell hides the differences (asynchronous browser startup, canvas sizing, WebGPU or WebGL 2 selection). Immediate-
//! mode UI (egui) draws HUDs and development tools on every target.

mod clock;
mod dev_tools;
mod egui_layer;
mod game;
#[cfg(not(target_arch = "wasm32"))]
mod headless;
mod input;
mod shell;
mod surface;
pub mod theme;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod web_input;

pub use clock::{DEFAULT_MAXIMUM_FRAME_SECONDS, FrameClock};
pub use dev_tools::DevTools;
pub use game::{Frame, FrameTime, Game, GameError, GameResult, RenderTarget, StartContext, Viewport};
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{HeadlessOptions, HeadlessShell};
pub use input::{ActionState, InputAxis, InputBinding, InputManager, InputMap, PointerState, key};
pub use shell::{ShellError, ShellOptions, run};

/// Immediate-mode UI used for HUDs and tools; games use it through this re-export.
pub use egui;
