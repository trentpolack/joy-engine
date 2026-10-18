// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine UI: the egui layer drawn over the rendered scene on native windows and browser canvases, the Joy visual
//! [`theme`], [`hud_text`] for in-game text, and the display metrics ([`Viewport`], [`display_scale`]) that layout,
//! picking, and painting share. Browsers translate DOM input into egui events themselves, because egui-winit's pinned
//! release does not build for WebAssembly.

mod display;
mod egui_layer;
pub mod hud_text;
pub mod theme;
#[cfg(target_arch = "wasm32")]
mod web_input;

pub use display::{
    Viewport,
    display_scale,
};

// Driven by the application shell and headless runner each frame; an engine contract rather than game-facing API.
#[doc(hidden)]
pub use egui_layer::{
    EguiLayer,
    paint_output,
};
