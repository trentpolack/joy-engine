// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The app contract: what a game, tool, or example implements to run in a shell, with its ordered frame phases, frame
//! timing, and the render target handed to the render phase.
//!
//! The runtime shell owns the window or canvas, GPU,
//! renderer, input, and development tools; the app implements phases that run in input, simulation, presentation,
//! render, UI, and after-frame order every frame. Time is in seconds.

use joy_devtools::DevTools;
use joy_input::InputManager;
use joy_renderer::{
    Camera,
    FrameInputs,
    GpuCapabilities,
    RenderError,
    RenderScene,
    Renderer,
    TriangleFrame,
    TriangleRenderer,
};
use joy_ui::Viewport;

/// Error type for app phases; any error converts into it with `?`.
pub type AppError = Box<dyn std::error::Error>;

/// Result returned by fallible app phases. An error stops the frame loop and shows the failure to the player.
pub type AppResult<T = ()> = Result<T, AppError>;

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

/// Requests an app can make of its shell.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ShellRequests {
    pub exit: bool,
    pub reset_clock: bool,
    pub pointer_lock: Option<bool>,
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

    /// Ask the shell to lock (hide and capture) or release the pointer after this frame, for mouse look. Locked
    /// pointers report movement through [`InputManager::pointer_motion`]; [`InputManager::pointer_locked`] reports
    /// whether the platform granted the lock. Browsers grant it only shortly after a click or key press and release it
    /// on Escape; native windows confine the cursor where locking is unsupported and release it when focus is lost.
    pub fn set_pointer_lock(&mut self, locked: bool) {
        self.requests.pointer_lock = Some(locked);
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

    /// Draw a frame with a app-owned [`TriangleRenderer`] (create it in [`App::start`] from
    /// `context.renderer.gpu()` and `context.renderer.output_format()`). The triangle renderer is resized to this target
    /// and given the shell's presentation clock first. Use either this or [`draw`](Self::draw) once per frame.
    pub fn draw_triangles(
        &mut self,
        triangles: &mut TriangleRenderer,
        frame: &TriangleFrame<'_>,
    ) -> Result<(), RenderError> {
        let (width, height) = self.renderer.physical_size();
        let logical = self.renderer.logical_size();
        if triangles.physical_size() != (width, height) || triangles.logical_size() != logical {
            let scale = if logical.x > 0.0 { width as f32 / logical.x } else { 1.0 };
            triangles.resize(width, height, scale);
        }
        triangles.set_presentation_time(self.renderer.presentation_time());
        if triangles.display_output() != self.renderer.display_output() {
            triangles.set_display_output(self.renderer.display_output());
        }
        triangles.encode(self.encoder, self.output, frame)?;
        self.drawn = true;
        Ok(())
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

/// A game, tool, or example driven by a shell ([`run`](crate::run) or [`HeadlessShell`](crate::HeadlessShell)). Every
/// phase has a default no-op except [`App::render`].
pub trait App: 'static {
    /// Called once when the GPU and renderer are ready.
    fn start(&mut self, _context: &mut StartContext<'_>) -> AppResult {
        Ok(())
    }

    /// Sample controls into player intent. Input has already been updated for this frame.
    fn input(&mut self, _frame: &mut Frame<'_>) -> AppResult {
        Ok(())
    }

    /// Advance simulation by `frame.time.elapsed_seconds`.
    fn simulation(&mut self, _frame: &mut Frame<'_>) -> AppResult {
        Ok(())
    }

    /// Synchronize presentation (scene instances, camera, audio) from simulation state.
    fn presentation(&mut self, _frame: &mut Frame<'_>) -> AppResult {
        Ok(())
    }

    /// Draw the frame into `target`.
    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult;

    /// Called when the window or page is hidden (`false`) or shown again (`true`). The shell has already reset the
    /// clock and released held input; games typically pause here.
    fn visibility_changed(&mut self, _visible: bool) {}

    /// The player asked to close the native window. Return `false` to keep running, for example to ask about unsaved
    /// work first and call [`Frame::request_exit`] once answered. Browsers never ask.
    fn close_requested(&mut self) -> bool {
        true
    }

    /// Build immediate-mode HUD and tool UI. Runs after rendering and draws over the scene.
    fn ui(&mut self, _context: &egui::Context, _frame: &mut Frame<'_>) {}

    /// Work after the frame is submitted (diagnostics, captures).
    fn after_frame(&mut self, _frame: &mut Frame<'_>) -> AppResult {
        Ok(())
    }
}
