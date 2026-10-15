// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The game contract: ordered frame phases, frame timing, and the render target handed to the render phase.
//!
//! This is the Rust counterpart of the TypeScript `GameShell` options: the shell owns the window or canvas, GPU,
//! renderer, input, and development tools; the game implements phases that run in input, simulation, presentation,
//! render, UI, and after-frame order every frame. Time is in seconds.

use glam::Vec2;
use joy_render::{Camera, FrameInputs, GpuCapabilities, RenderError, RenderScene, Renderer, wgpu};

use crate::dev_tools::DevTools;
use crate::input::InputManager;

/// Error type for game phases; any error converts into it with `?`.
pub type GameError = Box<dyn std::error::Error>;

/// Result returned by fallible game phases. An error stops the frame loop and shows the failure to the player.
pub type GameResult<T = ()> = Result<T, GameError>;

/// Timing for one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameTime {
    /// Seconds since the previous frame, clamped to the shell's maximum (0.1 by default) and zero after a pause,
    /// visibility change, or clock reset.
    pub elapsed_seconds: f32,
    /// Seconds since the shell started.
    pub time_seconds: f64,
    /// Zero-based frame counter.
    pub frame_index: u64,
}

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

/// Requests a game can make of its shell.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ShellRequests {
    pub exit: bool,
    pub reset_clock: bool,
}

/// Borrowed per-frame context passed to every phase.
pub struct Frame<'a> {
    /// This frame's timing.
    pub time: FrameTime,
    /// Current viewport.
    pub viewport: Viewport,
    /// Player input, already updated for this frame.
    pub input: &'a mut InputManager,
    /// Development overlay; publish diagnostic values with [`DevTools::set`].
    pub dev_tools: &'a mut DevTools,
    pub(crate) requests: &'a mut ShellRequests,
}

impl Frame<'_> {
    /// Ask the shell to close (native) after this frame. Browsers keep running.
    pub fn request_exit(&mut self) {
        self.requests.exit = true;
    }

    /// Reset elapsed time before resuming after a pause, so the next frame does not catch up.
    pub fn reset_clock(&mut self) {
        self.requests.reset_clock = true;
    }
}

/// The presentation target for the render phase. Draw the scene once per frame; an undrawn frame shows black.
pub struct RenderTarget<'a> {
    pub(crate) renderer: &'a mut Renderer,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
    pub(crate) output: &'a wgpu::TextureView,
    pub(crate) drawn: bool,
}

impl RenderTarget<'_> {
    /// Render `scene` from `camera` with this frame's lighting and transient geometry.
    pub fn draw(
        &mut self,
        scene: &RenderScene,
        camera: &impl Camera,
        inputs: &FrameInputs<'_>,
    ) -> Result<(), RenderError> {
        self.renderer.encode(self.encoder, scene, camera, inputs, self.output)?;
        self.drawn = true;
        Ok(())
    }

    /// The renderer, for display settings, picking, and diagnostics.
    pub fn renderer(&mut self) -> &mut Renderer {
        self.renderer
    }
}

/// Information available once the GPU is ready, before the first frame.
pub struct StartContext<'a> {
    /// Capabilities of the created device.
    pub capabilities: &'a GpuCapabilities,
    /// The renderer; adjust display settings here.
    pub renderer: &'a mut Renderer,
    /// Current viewport.
    pub viewport: Viewport,
}

/// A game driven by the shell. Every phase has a default no-op except [`Game::render`].
pub trait Game: 'static {
    /// Called once when the GPU and renderer are ready.
    fn start(&mut self, _context: &mut StartContext<'_>) -> GameResult {
        Ok(())
    }

    /// Sample controls into player intent. Input has already been updated for this frame.
    fn input(&mut self, _frame: &mut Frame<'_>) -> GameResult {
        Ok(())
    }

    /// Advance simulation by `frame.time.elapsed_seconds`.
    fn simulation(&mut self, _frame: &mut Frame<'_>) -> GameResult {
        Ok(())
    }

    /// Synchronize presentation (scene instances, camera, audio) from simulation state.
    fn presentation(&mut self, _frame: &mut Frame<'_>) -> GameResult {
        Ok(())
    }

    /// Draw the frame into `target`.
    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> GameResult;

    /// Called when the window or page is hidden (`false`) or shown again (`true`). The shell has already reset the
    /// clock and released held input; games typically pause here.
    fn visibility_changed(&mut self, _visible: bool) {}

    /// Build immediate-mode HUD and tool UI. Runs after rendering and draws over the scene.
    fn ui(&mut self, _context: &egui::Context, _frame: &mut Frame<'_>) {}

    /// Work after the frame is submitted (diagnostics, captures).
    fn after_frame(&mut self, _frame: &mut Frame<'_>) -> GameResult {
        Ok(())
    }
}
