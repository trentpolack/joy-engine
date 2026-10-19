// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Windowless frame runner for tests, deterministic captures, and tooling.
//!
//! Runs the same phases as the windowed shell with a fixed elapsed time per frame and an offscreen target, so app
//! behavior and rendering can be verified without a display. [`HeadlessShell::step`] skips the UI phase;
//! [`HeadlessShell::step_with_ui`] runs it with synthetic egui input (pointer, keys, text) and paints the UI over the
//! capture, so tools can be driven and screenshotted through their real interface. Native only.

use joy_devtools::DevTools;
use joy_input::{
    InputManager,
    InputMap,
};
use joy_renderer::{
    BackendPreference,
    GpuContext,
    OffscreenTarget,
    PresentationTime,
    RenderError,
    Renderer,
    RendererOptions,
};
use joy_ui::Viewport;

use crate::app::{
    App,
    AppResult,
    Frame,
    RenderTarget,
    ShellRequests,
    StartContext,
};
use crate::clock::FrameClock;
use crate::shell::ShellError;

/// Headless runner configuration.
#[derive(Clone, Debug)]
pub struct HeadlessOptions {
    /// Target width in pixels.
    pub width: u32,
    /// Target height in pixels.
    pub height: u32,
    /// Elapsed seconds simulated per frame.
    pub frame_seconds: f32,
    /// Renderer settings.
    pub renderer: RendererOptions,
    /// Player input actions and axes; drive them through [`HeadlessShell::input`].
    pub input: InputMap,
    /// Preferred graphics API.
    pub backend: BackendPreference,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            width: 640,
            height: 400,
            frame_seconds: 1.0 / 60.0,
            renderer: RendererOptions::default(),
            input: InputMap::new(),
            backend: BackendPreference::Auto,
        }
    }
}

/// A windowless shell that steps an app one frame at a time and reads back its pixels.
pub struct HeadlessShell {
    renderer: Renderer,
    target: OffscreenTarget,
    gpu: GpuContext,
    input: InputManager,
    dev_tools: DevTools,
    clock: FrameClock,
    requests: ShellRequests,
    frame_seconds: f64,
    time_seconds: f64,
    ui: Option<HeadlessUi>,
}

/// The UI context and painter, created on the first UI frame.
struct HeadlessUi {
    context: egui::Context,
    painter: egui_wgpu::Renderer,
}

impl HeadlessShell {
    /// Create a device, renderer, and offscreen target. Fails when no GPU adapter (hardware or software) exists.
    pub fn new(options: HeadlessOptions) -> Result<Self, ShellError> {
        let gpu = pollster::block_on(GpuContext::headless(options.backend))?;
        let target = OffscreenTarget::new(&gpu, options.width, options.height)?;
        let mut renderer = Renderer::new(&gpu, joy_renderer::CAPTURE_FORMAT, options.renderer);
        renderer.resize(options.width, options.height, 1.0);
        Ok(Self {
            renderer,
            target,
            gpu,
            input: InputManager::new(options.input),
            dev_tools: DevTools::new("headless", false),
            clock: FrameClock::new(f32::MAX),
            requests: ShellRequests::default(),
            frame_seconds: f64::from(options.frame_seconds),
            time_seconds: 0.0,
            ui: None,
        })
    }

    /// Call the app's start hook with this shell's renderer.
    pub fn start(&mut self, app: &mut impl App) -> AppResult {
        let viewport = self.viewport();
        let mut context =
            StartContext { capabilities: self.gpu.capabilities(), renderer: &mut self.renderer, viewport };
        app.start(&mut context)
    }

    /// Player input; set keys or virtual buttons before calling [`step`](Self::step).
    pub fn input(&mut self) -> &mut InputManager {
        &mut self.input
    }

    /// The renderer, for diagnostics and display settings.
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Run one frame's phases and render it offscreen. The UI phase is skipped.
    pub fn step(&mut self, app: &mut impl App) -> AppResult {
        self.run_frame(app, None).map(|_| ())
    }

    /// Run one frame including the UI phase with synthetic `input` (logical points; a missing `screen_rect` covers
    /// the whole target) and paint the UI over the capture, as the windowed shell does. Returns egui's platform output
    /// (copied text, cursor, opened URLs). UI text entry suspends gameplay input, as in the windowed shell.
    pub fn step_with_ui(&mut self, app: &mut impl App, input: egui::RawInput) -> AppResult<egui::PlatformOutput> {
        self.run_frame(app, Some(input)).map(Option::unwrap_or_default)
    }

    /// The UI context [`step_with_ui`](Self::step_with_ui) runs (created on first use with the engine theme), for
    /// inspecting focus, memory, and layout in tests.
    pub fn ui_context(&mut self) -> &egui::Context {
        &self.ensure_ui().context
    }

    fn ensure_ui(&mut self) -> &mut HeadlessUi {
        let gpu = &self.gpu;
        self.ui.get_or_insert_with(|| {
            let context = egui::Context::default();
            joy_ui::theme::apply(&context);
            let painter = egui_wgpu::Renderer::new(
                gpu.device(),
                joy_renderer::CAPTURE_FORMAT,
                egui_wgpu::RendererOptions::default(),
            );
            HeadlessUi { context, painter }
        })
    }

    fn run_frame(
        &mut self,
        app: &mut impl App,
        ui_input: Option<egui::RawInput>,
    ) -> AppResult<Option<egui::PlatformOutput>> {
        if ui_input.is_some() {
            self.ensure_ui();
        }
        let time = self.clock.tick(self.time_seconds);
        self.time_seconds += self.frame_seconds;
        if let Some(ui) = &self.ui {
            self.input.set_typing(ui.context.egui_wants_keyboard_input());
        }
        self.input.update();
        let viewport = self.viewport();
        let mut frame = Frame {
            time,
            viewport,
            input: &mut self.input,
            dev_tools: &mut self.dev_tools,
            requests: &mut self.requests,
        };

        app.input(&mut frame)?;
        app.simulation(&mut frame)?;
        app.presentation(&mut frame)?;

        // The fixed headless clock is also the presentation clock, so captures are repeatable.
        self.renderer.set_presentation_time(PresentationTime {
            time_seconds: time.time_seconds,
            elapsed_seconds: time.elapsed_seconds,
        });
        let mut encoder = self.gpu.device().create_command_encoder(&Default::default());
        let mut target = RenderTarget {
            renderer: &mut self.renderer,
            encoder: &mut encoder,
            output: self.target.view(),
            drawn: false,
        };
        app.render(&mut frame, &mut target)?;

        let mut platform_output = None;
        if let (Some(mut input), Some(ui)) = (ui_input, self.ui.as_mut()) {
            let (width, height) = self.target.size();
            input
                .screen_rect
                .get_or_insert(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32)));
            input.time.get_or_insert(time.time_seconds);
            input.predicted_dt = time.elapsed_seconds.max(1.0 / 60.0);
            let mut full_output = ui.context.run_ui(input, |root| app.ui(root.ctx(), &mut frame));
            platform_output = Some(std::mem::take(&mut full_output.platform_output));
            joy_ui::paint_output(
                &mut ui.painter,
                &self.gpu,
                &ui.context,
                &mut encoder,
                self.target.view(),
                [width, height],
                full_output,
            );
        }
        self.gpu.queue().submit([encoder.finish()]);
        app.after_frame(&mut frame)?;
        Ok(platform_output)
    }

    /// Run `count` frames.
    pub fn run_frames(&mut self, app: &mut impl App, count: u32) -> AppResult {
        for _ in 0..count {
            self.step(app)?;
        }
        Ok(())
    }

    /// Read back the latest frame as tightly packed RGBA8 rows.
    pub fn capture(&self) -> Result<Vec<u8>, RenderError> {
        self.target.read_rgba(&self.gpu)
    }

    /// Write the latest frame to a PNG file.
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), ShellError> {
        let pixels = self.capture()?;
        let (width, height) = self.target.size();
        image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8)
            .map_err(|error| ShellError::Graphics(RenderError::Capture(error.to_string())))
    }

    fn viewport(&self) -> Viewport {
        Viewport { logical_size: self.renderer.logical_size(), scale_factor: 1.0 }
    }
}
