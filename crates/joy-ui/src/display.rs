// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Display metrics shared by the shell, the egui layer, and development tools: the logical viewport and how many
//! physical pixels each logical (CSS) pixel covers.

use glam::Vec2;
use winit::window::Window;

/// Viewport dimensions; logical (CSS) pixels drive layout and picking, never display density.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Viewport {
    /// Size in logical pixels.
    pub logical_size: Vec2,
    /// Physical pixels per logical pixel.
    pub scale_factor: f32,
}

impl Viewport {
    /// Width divided by height, or one for an empty viewport.
    pub fn aspect(&self) -> f32 {
        if self.logical_size.y > 0.0 { self.logical_size.x / self.logical_size.y } else { 1.0 }
    }
}

/// Physical pixels per logical pixel for layout and rendering.
///
/// Browsers measure the canvas backing store and `devicePixelRatio` separately, and the two disagree under some zoom and
/// emulation settings; the ratio of the canvas's backing size to its CSS size is what is actually displayed, so the web
/// uses it. Pointer coordinates keep winit's own ratio, which is how winit encoded them.
pub fn display_scale(window: &Window) -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        canvas_scale(window)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        window.scale_factor() as f32
    }
}

/// Backing-store pixels per CSS pixel of the window's canvas, falling back to `devicePixelRatio` before layout.
#[cfg(target_arch = "wasm32")]
fn canvas_scale(window: &winit::window::Window) -> f32 {
    use winit::platform::web::WindowExtWebSys;
    let physical_width = window.inner_size().width;
    match window.canvas() {
        Some(canvas) if canvas.client_width() > 0 && physical_width > 0 => {
            physical_width as f32 / canvas.client_width() as f32
        }
        _ => window.scale_factor() as f32,
    }
}
