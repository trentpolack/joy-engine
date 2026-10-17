// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The SDF collisions demo: two bins side by side fill with the same FORM props, dropped in the same poses on the same
//! schedule. The left bin collides through signed distance fields; the right through the props' bounding boxes. Both
//! simulations step every frame, so the difference in how the piles settle, and what each costs, is visible at once.
//!
//! Drag orbits, the wheel zooms, Space drops more props, P pauses, and R empties both bins and starts over.

use joy_engine::prelude::*;
use joy_engine::rendering::ShiftedCamera;
use joy_engine::runtime::{
    Stopwatch,
    launch_parameter
};

use super::benchmark::{
    BenchmarkReport,
    run_benchmark
};
use super::config::sdf_collisions_config::{
    ARENA,
    BENCHMARK,
    PRESENTATION,
    SPAWN,
    VIEW
};
use super::hud::{
    self,
    HudCommand,
    HudView,
    sidebar_width
};
use super::pile_arena::{
    ArenaSnapshot,
    ColliderMode,
    PileArena
};
use super::prop_library::PropLibrary;

/// Input action held to orbit (primary mouse button or a touch).
pub const ORBIT_ACTION: &str = "orbit";
/// Input actions for DROP MORE, PAUSE, and RESET.
pub const DROP_ACTION: &str = "drop";
pub const PAUSE_ACTION: &str = "pause";
pub const RESET_ACTION: &str = "reset";

/// The demo's input: drag with the primary button (or a touch), Space, P, and R.
pub fn input_map() -> InputMap {
    InputMap::new()
        .action(ORBIT_ACTION, [InputBinding::MouseButton(0)])
        .action(DROP_ACTION, [InputBinding::key(key::SPACE)])
        .action(PAUSE_ACTION, [InputBinding::key(key::KEY_P)])
        .action(RESET_ACTION, [InputBinding::key(key::KEY_R)])
}

/// Whether the launch parameters ask for the benchmark (`--bench` natively, `?bench=1` in browsers).
pub fn benchmark_requested() -> bool {
    launch_parameter(BENCHMARK.parameter).is_some()
}

/// Run the configured benchmark and return its one-line report.
pub fn benchmark(library: &PropLibrary) -> GameResult<BenchmarkReport> {
    Ok(run_benchmark(library, BENCHMARK.steps, BENCHMARK.props)?)
}

/// Copied diagnostics of both bins.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code, reason = "part of the capture and test API that test/rust drives")]
pub struct DemoSnapshot {
    /// The SDF bin.
    pub sdf: ArenaSnapshot,
    /// The bounding-box bin.
    pub boxes: ArenaSnapshot,
    /// Whether both simulations are paused.
    pub paused: bool,
    /// Frames rendered.
    pub frames: u64,
}

/// The demo: owns the scene, the prop library, both bins, the camera, and HUD state.
pub struct SdfCollisionsDemo {
    // Content and simulation. Bins borrow the library's shapes and meshes per call.
    scene: RenderScene,
    library: PropLibrary,
    sdf_bin: PileArena,
    box_bin: PileArena,
    paused: bool,

    // Presentation and input.
    camera: OrbitCamera,
    drag: Option<Vec2>,
    reset_history: bool,

    // Diagnostics: CPU render-encoding samples in milliseconds, the backend label, and the benchmark line.
    frame_count: u64,
    render_samples: Vec<f64>,
    backend: &'static str,
    benchmark: Option<String>,
}

impl SdfCollisionsDemo {
    /// Both empty bins and the evaluated props.
    pub fn new() -> GameResult<Self> {
        let library = PropLibrary::load()?;
        let mut scene = RenderScene::new();
        let half_separation = ARENA.separation * 0.5;
        let sdf_bin = PileArena::new(ColliderMode::Sdf, -half_separation, Some(&mut scene))?;
        let box_bin = PileArena::new(ColliderMode::BoundingBox, half_separation, Some(&mut scene))?;
        let camera = OrbitCamera {
            target: VIEW.target,
            distance: VIEW.distance,
            yaw: VIEW.yaw,
            pitch: VIEW.pitch,
            near_clip: Some(VIEW.clips.0),
            far_clip: Some(VIEW.clips.1),
            ..OrbitCamera::default()
        };
        Ok(Self {
            scene,
            library,
            sdf_bin,
            box_bin,
            paused: false,
            camera,
            drag: None,
            reset_history: false,
            frame_count: 0,
            render_samples: Vec::new(),
            backend: "starting",
            benchmark: None,
        })
    }

    /// Advance both bins by `elapsed_seconds` (bounded per frame) unless paused.
    pub fn advance(&mut self, elapsed_seconds: f32) -> GameResult {
        if self.paused {
            return Ok(());
        }
        let elapsed = elapsed_seconds.min(PRESENTATION.maximum_frame_seconds);
        self.sdf_bin.update(elapsed, &self.library, Some(&mut self.scene))?;
        self.box_bin.update(elapsed, &self.library, Some(&mut self.scene))?;
        Ok(())
    }

    /// Grow both piles by a burst of props.
    pub fn drop_more(&mut self) {
        self.sdf_bin.raise_target(SPAWN.burst);
        self.box_bin.raise_target(SPAWN.burst);
    }

    /// Empty both bins and restart the shared drop sequence.
    pub fn reset(&mut self) {
        self.sdf_bin.reset(Some(&mut self.scene));
        self.box_bin.reset(Some(&mut self.scene));
        self.reset_history = true;
    }

    /// Pause or resume both simulations.
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Copied diagnostics.
    #[allow(dead_code, reason = "part of the capture and test API that test/rust drives")]
    pub fn snapshot(&self) -> DemoSnapshot {
        DemoSnapshot {
            sdf: self.sdf_bin.snapshot(),
            boxes: self.box_bin.snapshot(),
            paused: self.paused,
            frames: self.frame_count,
        }
    }

    fn apply(&mut self, command: HudCommand) {
        match command {
            HudCommand::DropMore => self.drop_more(),
            HudCommand::TogglePause => self.toggle_pause(),
            HudCommand::Reset => self.reset(),
        }
    }

    /// The orbit camera with its image centered beside the sidebar.
    fn view_camera(&self, viewport: Vec2) -> ShiftedCamera<'_, OrbitCamera> {
        ShiftedCamera::between_insets(&self.camera, viewport.x, 0.0, sidebar_width(viewport.x))
    }
}

impl Game for SdfCollisionsDemo {
    /// Record the backend, and run the benchmark when the launch parameters request it. It runs here rather than in
    /// [`SdfCollisionsDemo::new`] because the shell routes logs to the console only once it starts.
    fn start(&mut self, context: &mut StartContext<'_>) -> GameResult {
        self.backend = context.capabilities.backend.label();
        if benchmark_requested() {
            let report = benchmark(&self.library)?;
            joy_engine::log::info!("{report}");
            self.benchmark = Some(report.to_string());
        }
        Ok(())
    }

    fn input(&mut self, frame: &mut Frame<'_>) -> GameResult {
        if frame.input.pressed(DROP_ACTION) {
            self.drop_more();
        }
        if frame.input.pressed(PAUSE_ACTION) {
            self.toggle_pause();
        }
        if frame.input.pressed(RESET_ACTION) {
            self.reset();
        }

        let pointer = frame.input.pointer();
        if frame.input.action(ORBIT_ACTION).down && pointer.active {
            if let Some(previous) = self.drag {
                let delta = pointer.position - previous;
                self.camera.yaw -= delta.x * VIEW.orbit_sensitivity;
                self.camera.pitch = (self.camera.pitch + (delta.y * VIEW.orbit_sensitivity))
                    .clamp(VIEW.pitch_limits.0, VIEW.pitch_limits.1);
            }
            self.drag = Some(pointer.position);
        } else {
            self.drag = None;
        }
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> GameResult {
        self.advance(frame.time.elapsed_seconds)
    }

    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> GameResult {
        if std::mem::take(&mut self.reset_history) {
            target.renderer().reset_history();
        }
        let lights = [PRESENTATION.key_light, PRESENTATION.fill_light];
        let inputs = FrameInputs {
            lights: &lights,
            ambient: Some(PRESENTATION.ambient),
            clear_color: PRESENTATION.clear_color,
            time_seconds: frame.time.time_seconds as f32,
            ..FrameInputs::default()
        };
        let camera = self.view_camera(frame.viewport.logical_size);
        let stopwatch = Stopwatch::start();
        target.draw(&self.scene, &camera, &inputs)?;
        self.render_samples.push(stopwatch.elapsed_milliseconds());
        if self.render_samples.len() > PRESENTATION.timing_samples {
            self.render_samples.remove(0);
        }
        self.frame_count += 1;

        let sdf = self.sdf_bin.snapshot();
        let boxes = self.box_bin.snapshot();
        frame.dev_tools.set("SDF bin", format!("{} props · {:.2} ms/step", sdf.bodies, sdf.mean_step_milliseconds));
        frame.dev_tools.set("box bin", format!("{} props · {:.2} ms/step", boxes.bodies, boxes.mean_step_milliseconds));
        frame
            .dev_tools
            .set("SDF contacts", format!("{} pairs · {} contacts", sdf.statistics.pairs, sdf.statistics.contacts));
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        let sdf = self.sdf_bin.snapshot();
        let boxes = self.box_bin.snapshot();
        let render_milliseconds = if self.render_samples.is_empty() {
            0.0
        } else {
            self.render_samples.iter().sum::<f64>() / self.render_samples.len() as f64
        };
        let view = HudView {
            sdf: &sdf,
            boxes: &boxes,
            props: self.library.props(),
            field_bytes: self.library.field_bytes(),
            paused: self.paused,
            backend: self.backend,
            render_milliseconds,
            benchmark: self.benchmark.as_deref(),
        };
        for command in hud::draw(context, &view) {
            self.apply(command);
        }

        // Wheel zoom belongs to the canvas, not to the scrolling sidebar.
        let scroll = context.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0 && !context.is_pointer_over_egui() {
            let distance = self.camera.distance * (-scroll * VIEW.zoom_sensitivity).exp();
            self.camera.distance = distance.clamp(VIEW.distance_limits.0, VIEW.distance_limits.1);
        }
    }
}
