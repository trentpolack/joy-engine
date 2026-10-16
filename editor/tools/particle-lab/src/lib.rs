// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! PARTICLE LAB on the Rust Joy Engine: author deterministic, layered `.joyfx` particle effects with scripts, child
//! emitters, textures, lights, and a live preview drawn by the engine renderer.
//!
//! Runs natively (`cargo run -p particle-lab`) and builds for browsers with
//! `npm run build --workspace @joy-games/particle-lab`. Joy Editor hosts [`ParticleLabTool`] as a tab through
//! [`AuthoringTool`](joy_engine::development::AuthoringTool). Module responsibilities are split by model and UI:
//! document, examples, storage, and session logic stay free of interface code; `editor`, `emitter_inspector`,
//! `parameter_controls`, and `interface` are the egui interface; `viewport` draws through the engine.

pub mod constants;
pub mod document;
pub mod editor;
pub mod emitter_inspector;
pub mod examples;
pub mod interface;
pub mod lab;
pub mod parameter_controls;
pub mod storage;
pub mod tool;
pub mod viewport;

use joy_engine::{ShellError, ShellOptions};

pub use tool::ParticleLabTool;

/// Browser canvas element ID.
pub const CANVAS_ID: &str = "joy-canvas";

/// Shell configuration shared by the native window and the browser canvas.
pub fn shell_options() -> ShellOptions {
    ShellOptions {
        title: tool::TOOL_NAME.to_owned(),
        canvas_id: CANVAS_ID.to_owned(),
        development: false,
        ..Default::default()
    }
}

/// Run PARTICLE LAB until the window closes (native) or for the life of the page (browser).
pub fn run() -> Result<(), ShellError> {
    joy_engine::run(shell_options(), ParticleLabTool::new())
}

/// Browser entry point.
#[cfg(all(target_arch = "wasm32", feature = "web-entry"))]
fn start() {
    if let Err(error) = run() {
        joy_engine::log::error!("{error}");
    }
}

#[cfg(feature = "web-entry")]
joy_engine::web_main!(start);
