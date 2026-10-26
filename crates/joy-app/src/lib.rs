// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The application shell: the window (native) or canvas (browser) that owns the GPU and renderer, input and gamepads,
//! the frame clock, egui, and the development overlay, and runs an [`App`]'s phases every frame.
//!
//! The same app code runs as a native application (Windows, macOS, Linux) and in the browser through WebAssembly; the
//! shell hides the differences (asynchronous browser startup, canvas sizing, WebGPU or WebGL 2 selection). Games,
//! tools, and examples implement [`App`] and hand it to [`run`]; [`HeadlessShell`] runs the same phases offscreen for
//! tests, captures, and tooling. [`Stopwatch`] times CPU work on every target, and [`launch_parameter`] reads the same
//! development switches from a URL query or the command line. [`AssetFetcher`] reads streamed assets (terrain package
//! tiles, for example) without blocking a frame: from a folder on a worker thread natively, over HTTP in browsers.
//!
//! The shell composes the other platform crates (`joy-input`, `joy-ui`, and `joy-devtools`) with `joy-renderer`; apps
//! reach all of them through `joy_engine::runtime`.

mod app;
#[cfg(not(target_arch = "wasm32"))]
mod asset_fetch;
mod clock;
#[cfg(not(target_arch = "wasm32"))]
mod headless;
mod launch;
mod shell;
mod surface;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod web_asset_fetch;

pub use app::{
    App,
    AppError,
    AppResult,
    Frame,
    FrameTime,
    RenderTarget,
    StartContext,
};
#[cfg(not(target_arch = "wasm32"))]
pub use asset_fetch::AssetFetcher;
pub use clock::{
    DEFAULT_MAXIMUM_FRAME_SECONDS,
    FrameClock,
    Stopwatch,
    unix_time_milliseconds,
};
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{
    CAPTURE_DIRECTORY_VARIABLE,
    HeadlessOptions,
    HeadlessShell,
    region_mean_color,
};
pub use launch::launch_parameter;
pub use shell::{
    ShellError,
    ShellOptions,
    run,
};
#[cfg(target_arch = "wasm32")]
pub use web_asset_fetch::AssetFetcher;
