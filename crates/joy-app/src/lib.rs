// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The application shell: the window (native) or canvas (browser) that owns the GPU and renderer, input and gamepads,
//! the frame clock, egui, and the development overlay, and runs a [`Game`]'s phases every frame.
//!
//! The same game code runs as a native application (Windows, macOS, Linux) and in the browser through WebAssembly; the
//! shell hides the differences (asynchronous browser startup, canvas sizing, WebGPU or WebGL 2 selection). Games
//! implement [`Game`] and hand it to [`run`]; [`HeadlessShell`] runs the same phases offscreen for tests, captures, and
//! tooling. [`FixedStepLoop`] runs fixed-rate simulation inside variable-rate frames, [`Stopwatch`] times CPU work on
//! every target, and [`launch_parameter`] reads the same development switches from a URL query or the command line.
//!
//! The shell composes the other platform crates (`joy-input`, `joy-ui`, and `joy-devtools`) with `joy-renderer`; games
//! reach all of them through `joy_engine::runtime`.

mod clock;
mod game;
mod game_loop;
#[cfg(not(target_arch = "wasm32"))]
mod headless;
mod launch;
mod shell;
mod surface;
#[cfg(target_arch = "wasm32")]
mod web;

pub use clock::{
    DEFAULT_MAXIMUM_FRAME_SECONDS,
    FrameClock,
    Stopwatch,
    unix_time_milliseconds,
};
pub use game::{
    Frame,
    FrameTime,
    Game,
    GameError,
    GameResult,
    RenderTarget,
    StartContext,
};
pub use game_loop::{
    DEFAULT_UPDATES_PER_SECOND,
    FixedStepLoop,
    MAXIMUM_FRAME_SECONDS,
};
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{
    HeadlessOptions,
    HeadlessShell,
};
pub use launch::launch_parameter;
pub use shell::{
    ShellError,
    ShellOptions,
    run,
};
