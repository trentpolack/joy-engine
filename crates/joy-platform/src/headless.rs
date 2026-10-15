// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Windowless frame runner for tests, deterministic captures, and tooling.
//!
//! Runs the same phases as the windowed shell with a fixed elapsed time per frame and an offscreen target, so game
//! behavior and rendering can be verified without a display. The UI phase is skipped. Native only.

use joy_render::{BackendPreference, GpuContext, OffscreenTarget, RenderError, Renderer, RendererOptions};

use crate::clock::FrameClock;
use crate::dev_tools::DevTools;
use crate::game::{Frame, Game, GameResult, RenderTarget, ShellRequests, StartContext, Viewport};
use crate::input::{InputManager, InputMap};
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

/// A windowless shell that steps a game one frame at a time and reads back its pixels.
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
}

impl HeadlessShell {
    /// Create a device, renderer, and offscreen target. Fails when no GPU adapter (hardware or software) exists.
    pub fn new(options: HeadlessOptions) -> Result<Self, ShellError> {
        let gpu = pollster::block_on(GpuContext::headless(options.backend))?;
        let target = OffscreenTarget::new(&gpu, options.width, options.height)?;
        let mut renderer = Renderer::new(&gpu, joy_render::CAPTURE_FORMAT, options.renderer);
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
        })
    }

    /// Call the game's start hook with this shell's renderer.
    pub fn start(&mut self, game: &mut impl Game) -> GameResult {
        let viewport = self.viewport();
        let mut context =
            StartContext { capabilities: self.gpu.capabilities(), renderer: &mut self.renderer, viewport };
        game.start(&mut context)
    }

    /// Player input; set keys or virtual buttons before calling [`step`](Self::step).
    pub fn input(&mut self) -> &mut InputManager {
        &mut self.input
    }

    /// The renderer, for diagnostics and display settings.
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Run one frame's phases and render it offscreen.
    pub fn step(&mut self, game: &mut impl Game) -> GameResult {
        let time = self.clock.tick(self.time_seconds);
        self.time_seconds += self.frame_seconds;
        self.input.update();
        let viewport = self.viewport();
        let mut frame = Frame {
            time,
            viewport,
            input: &mut self.input,
            dev_tools: &mut self.dev_tools,
            requests: &mut self.requests,
        };

        game.input(&mut frame)?;
        game.simulation(&mut frame)?;
        game.presentation(&mut frame)?;

        let mut encoder = self.gpu.device().create_command_encoder(&Default::default());
        let mut target = RenderTarget {
            renderer: &mut self.renderer,
            encoder: &mut encoder,
            output: self.target.view(),
            drawn: false,
        };
        game.render(&mut frame, &mut target)?;
        self.gpu.queue().submit([encoder.finish()]);
        game.after_frame(&mut frame)
    }

    /// Run `count` frames.
    pub fn run_frames(&mut self, game: &mut impl Game, count: u32) -> GameResult {
        for _ in 0..count {
            self.step(game)?;
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
