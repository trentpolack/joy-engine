// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The scene effects lab: one deterministic room drawn by the engine renderer, the engine's scene effects preview
//! controls with the lab's tuning, temporal tests (motion, a frozen or stepped clock, camera cuts, scene replacement,
//! and history resets), and CPU timing readouts.
//!
//! Simulation starts frozen at two seconds while frames keep rendering, so temporal effects accumulate history on a
//! still image. The sidebar covers the right of the canvas; a lens shift keeps the room centered in the rest. Volumetric
//! fog needs compute shaders; on WebGL 2 the controls report it unavailable while the other effects and HDR bloom run.

use joy_engine::development::{
    EffectPreset,
    EffectQuality,
    SceneEffectsControls,
    dock,
};
use joy_engine::egui::{
    Align2,
    Color32,
    FontId,
    Id,
    Margin,
    RichText,
    Stroke,
    Vec2 as UiVec2,
};
use joy_engine::math::deterministic;
use joy_engine::prelude::*;
use joy_engine::rendering::scene_effects::SceneEffectsOptions;
use joy_engine::rendering::{
    RendererDiagnostics,
    ShiftedCamera,
};
use joy_engine::runtime::Stopwatch;

use super::config::{
    AMBIENT,
    BLOOM_STRENGTH,
    CAMERA_CLIPS,
    CAMERA_CUT_YAW,
    CAMERA_DISTANCE,
    CAMERA_DISTANCE_LIMITS,
    CAMERA_MOTION,
    CAMERA_PITCH,
    CAMERA_PITCH_LIMITS,
    CAMERA_RADIUS,
    CAMERA_TARGET,
    CAMERA_YAW,
    CLEAR_COLOR,
    EFFECT_NOTES,
    MAXIMUM_STEP_SECONDS,
    OBJECT_MOTION,
    ORBIT_SENSITIVITY,
    START_SECONDS,
    STATS_INTERVAL_FRAMES,
    STEP_SECONDS,
    TIMING_SAMPLES,
    ZOOM_SENSITIVITY,
    post_process_settings,
    quality_config,
};
use super::room::{
    MOVING_POSITION,
    Room,
    create_room,
};

/// Input action held to orbit (primary mouse button or a touch).
pub const ORBIT_ACTION: &str = "orbit";

/// Sidebar width in logical pixels on wide viewports, and on viewports at most [`MEDIUM_VIEWPORT`] wide.
const SIDEBAR_WIDTH: f32 = 310.0;
const SIDEBAR_WIDTH_MEDIUM: f32 = 260.0;
const MEDIUM_VIEWPORT: f32 = 850.0;
/// Viewports narrower than this stack the controls in a bottom sheet of [`SHEET_FRACTION`] of the height.
const NARROW_VIEWPORT: f32 = 600.0;
const SHEET_FRACTION: f32 = 0.4;
/// Overlay text inset from the viewport edges.
const OVERLAY_MARGIN: UiVec2 = UiVec2::new(30.0, 24.0);

// The lab's palette.
const SIDEBAR_FILL: Color32 = Color32::from_rgb(0x19, 0x1e, 0x29);
const SIDEBAR_LINE: Color32 = Color32::from_rgb(0x2b, 0x33, 0x43);
const TEXT: Color32 = Color32::from_rgb(0xe8, 0xed, 0xf5);
const MUTED: Color32 = Color32::from_rgb(0xb7, 0xc0, 0xd3);
const DETAIL: Color32 = Color32::from_rgb(0x9b, 0xa9, 0xc0);
const ACCENT: Color32 = Color32::from_rgb(0xfe, 0xa4, 0x65);

/// A user request from the sidebar, applied after drawing it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LabCommand {
    Baseline,
    Combined,
    Motion { object: bool, camera: bool },
    Paused(bool),
    Time(f64),
    Step,
    CameraCut,
    ReplaceScene,
    ResetHistory,
}

/// Where the controls cover the canvas, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// Sidebar width at the right edge (zero when stacked).
    right: f32,
    /// Bottom sheet height (zero beside a sidebar).
    bottom: f32,
}

impl Layout {
    /// The responsive layout: a 310-pixel sidebar, 260 pixels at medium widths, and a stacked sheet on phones.
    fn for_viewport(size: Vec2) -> Self {
        if size.x < NARROW_VIEWPORT {
            Self { right: 0.0, bottom: (size.y * SHEET_FRACTION).round() }
        } else if size.x <= MEDIUM_VIEWPORT {
            Self { right: SIDEBAR_WIDTH_MEDIUM, bottom: 0.0 }
        } else {
            Self { right: SIDEBAR_WIDTH, bottom: 0.0 }
        }
    }
}

/// Copied lab state: settings, camera heading, room counts, and the latest renderer statistics. Times are seconds.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code, reason = "part of the capture API that the integration tests drive")]
pub struct LabSnapshot {
    /// The scene effects the controls request.
    pub effects: SceneEffectsOptions,
    /// Selected effect quality.
    pub quality: EffectQuality,
    /// Whether the simulation clock is frozen.
    pub paused: bool,
    /// Whether the orange block moves.
    pub object_motion: bool,
    /// Whether the camera sways.
    pub camera_motion: bool,
    /// Simulation time.
    pub time_seconds: f64,
    /// The latest simulation step.
    pub delta_seconds: f64,
    /// Whether the alternate room colors are showing.
    pub alternate_room: bool,
    /// Camera heading in radians.
    pub camera_yaw: f32,
    /// Point lights in the room.
    pub lights: usize,
    /// Point lights set to cast shadows.
    pub casting_lights: usize,
    /// Mesh instances in the room.
    pub meshes: usize,
    /// Renderer statistics from the latest frame.
    pub diagnostics: RendererDiagnostics,
    /// Frames rendered.
    pub frames: u64,
}

/// The lab: owns the room, camera, preview controls, clock, and timing samples. Dropping it releases the room.
pub struct SceneEffectsExample {
    // Scene and view.
    room: Room,
    camera: OrbitCamera,
    controls: SceneEffectsControls,

    // Temporal test state; times are seconds.
    paused: bool,
    object_motion: bool,
    camera_motion: bool,
    time_seconds: f64,
    delta_seconds: f64,
    camera_offset: f32,
    alternate_room: bool,
    reset_requested: bool,

    // Input.
    drag: Option<Vec2>,

    // Bounded diagnostic samples in milliseconds; neither measures GPU execution.
    encode_samples: Vec<f64>,
    cadence_samples: Vec<f64>,
    frames: u64,
    diagnostics: RendererDiagnostics,
    physical_size: (u32, u32),
    color_format: String,
    stats: String,
    capabilities: String,
}

impl Default for SceneEffectsExample {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneEffectsExample {
    /// The lab with every effect off, the clock frozen at two seconds, and the default camera.
    pub fn new() -> Self {
        let camera = OrbitCamera {
            target: CAMERA_TARGET,
            radius: CAMERA_RADIUS,
            distance: CAMERA_DISTANCE,
            near_clip: Some(CAMERA_CLIPS.0),
            far_clip: Some(CAMERA_CLIPS.1),
            yaw: CAMERA_YAW,
            pitch: CAMERA_PITCH,
            ..OrbitCamera::default()
        };
        let controls = SceneEffectsControls::new()
            .with_tuning(quality_config)
            .with_bloom_strength(BLOOM_STRENGTH)
            .with_default_open(true);
        Self {
            room: create_room(false),
            camera,
            controls,
            paused: true,
            object_motion: false,
            camera_motion: false,
            time_seconds: START_SECONDS,
            delta_seconds: STEP_SECONDS,
            camera_offset: 0.0,
            alternate_room: false,
            reset_requested: true,
            drag: None,
            encode_samples: Vec::new(),
            cadence_samples: Vec::new(),
            frames: 0,
            diagnostics: RendererDiagnostics::default(),
            physical_size: (0, 0),
            color_format: "unknown".to_owned(),
            stats: "Initializing renderer…".to_owned(),
            capabilities: "Awaiting device…".to_owned(),
        }
    }

    /// Turn deterministic sine motion of the block and the camera on or off.
    pub fn set_motion(&mut self, object: bool, camera: bool) {
        self.object_motion = object;
        self.camera_motion = camera;
        self.reset_history();
    }

    /// Freeze or resume the simulation clock.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Jump to a simulation time in seconds; negative or non-finite times are ignored.
    pub fn set_time(&mut self, seconds: f64) {
        if seconds.is_finite() && seconds >= 0.0 {
            self.time_seconds = seconds;
            self.reset_history();
        }
    }

    /// Advance the simulation clock by one sixtieth of a second, keeping temporal history.
    pub fn step(&mut self) {
        self.time_seconds += STEP_SECONDS;
        self.delta_seconds = STEP_SECONDS;
    }

    /// Toggle a heading offset large enough that the renderer restarts temporal history.
    pub fn camera_cut(&mut self) {
        self.camera_offset = if self.camera_offset == 0.0 { CAMERA_CUT_YAW } else { 0.0 };
        self.camera.yaw = CAMERA_YAW + self.camera_offset;
        self.reset_history();
    }

    /// Replace the room with the other wall colors; the previous scene is dropped.
    pub fn replace_scene(&mut self) {
        self.alternate_room = !self.alternate_room;
        self.room = create_room(self.alternate_room);
        self.reset_history();
    }

    /// Restart temporal effect history at the next frame.
    pub fn reset_history(&mut self) {
        self.reset_requested = true;
    }

    fn apply(&mut self, command: LabCommand) {
        match command {
            LabCommand::Baseline => self.controls.select_preset(EffectPreset::Off),
            LabCommand::Combined => self.controls.select_preset(EffectPreset::Combined),
            LabCommand::Motion { object, camera } => self.set_motion(object, camera),
            LabCommand::Paused(paused) => self.set_paused(paused),
            LabCommand::Time(seconds) => self.set_time(seconds),
            LabCommand::Step => self.step(),
            LabCommand::CameraCut => self.camera_cut(),
            LabCommand::ReplaceScene => self.replace_scene(),
            LabCommand::ResetHistory => self.reset_history(),
        }
    }

    /// The orbit camera with its image centered beside (or above) the controls.
    fn view_camera(&self, viewport: Vec2) -> ShiftedCamera<'_, OrbitCamera> {
        let layout = Layout::for_viewport(viewport);
        let shift = if viewport.x > 0.0 && viewport.y > 0.0 {
            Vec2::new(-layout.right / viewport.x, layout.bottom / viewport.y)
        } else {
            Vec2::ZERO
        };
        ShiftedCamera { camera: &self.camera, shift }
    }

    fn refresh_stats(&mut self) {
        let (width, height) = self.physical_size;
        let lighting = if self.diagnostics.deferred { "deferred" } else { "forward" };
        self.stats = format!(
            "{} · {} · {width} × {height} · {lighting} lighting\n{} meshes / {} lights / t={:.3} s\nCPU frame encoding \
             {:.2} ms · frame cadence {:.2} ms\nGPU execution: unavailable",
            self.diagnostics.backend,
            self.color_format,
            self.room.scene.len(),
            self.room.lights.len(),
            self.time_seconds,
            mean(&self.encode_samples),
            mean(&self.cadence_samples),
        );
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Interface
    // -----------------------------------------------------------------------------------------------------------------

    /// The CONFIG, editor, and capabilities sections, beside the canvas or stacked beneath it.
    fn sidebar(&mut self, context: &egui::Context, layout: Layout, commands: &mut Vec<LabCommand>) {
        let screen = context.content_rect();
        let (anchor, size) = if layout.bottom > 0.0 {
            (Align2::LEFT_BOTTOM, UiVec2::new(screen.width(), layout.bottom))
        } else {
            (Align2::RIGHT_TOP, UiVec2::new(layout.right, screen.height()))
        };
        let margin = if layout.right == SIDEBAR_WIDTH { 18 } else { 12 };
        let reserved = dock::reserved_height(context);
        egui::Area::new(Id::new("scene-effects-sidebar")).anchor(anchor, UiVec2::ZERO).constrain(false).show(
            context,
            |ui| {
                egui::Frame::new()
                    .fill(SIDEBAR_FILL)
                    .stroke(Stroke::new(1.0, SIDEBAR_LINE))
                    .inner_margin(Margin::same(margin))
                    .show(ui, |ui| {
                        let inner = size - UiVec2::splat(f32::from(margin) * 2.0);
                        ui.set_width(inner.x);
                        ui.set_height(inner.y);
                        ui.visuals_mut().override_text_color = Some(TEXT);
                        ui.visuals_mut().selection.bg_fill = ACCENT;
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            self.sidebar_contents(ui, commands);
                            // The development dock sits over the bottom-right corner.
                            ui.add_space(reserved);
                        });
                    });
            },
        );
    }

    fn sidebar_contents(&mut self, ui: &mut egui::Ui, commands: &mut Vec<LabCommand>) {
        egui::CollapsingHeader::new(RichText::new("CONFIG").strong()).default_open(true).show(ui, |ui| {
            self.controls.ui(ui);
            ui.horizontal(|ui| {
                if ui.button("All off").clicked() {
                    commands.push(LabCommand::Baseline);
                }
                if ui.button("Combined").clicked() {
                    commands.push(LabCommand::Combined);
                }
            });

            ui.add_space(10.0);
            ui.label(RichText::new("Temporal tests").strong().color(TEXT));
            let (mut object, mut camera) = (self.object_motion, self.camera_motion);
            let object_changed = ui.checkbox(&mut object, "Moving object").changed();
            let camera_changed = ui.checkbox(&mut camera, "Camera motion").changed();
            if object_changed || camera_changed {
                commands.push(LabCommand::Motion { object, camera });
            }
            let mut paused = self.paused;
            if ui.checkbox(&mut paused, "Fixed simulation time").changed() {
                commands.push(LabCommand::Paused(paused));
            }
            ui.horizontal(|ui| {
                ui.label("Time (seconds)");
                let mut seconds = self.time_seconds;
                let field =
                    egui::DragValue::new(&mut seconds).speed(STEP_SECONDS).range(0.0..=f64::MAX).fixed_decimals(4);
                if ui.add(field).changed() {
                    commands.push(LabCommand::Time(seconds));
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (label, command) in [
                    ("Step 1/60 s", LabCommand::Step),
                    ("Camera cut", LabCommand::CameraCut),
                    ("Replace scene", LabCommand::ReplaceScene),
                    ("Reset history", LabCommand::ResetHistory),
                ] {
                    if ui.button(label).clicked() {
                        commands.push(command);
                    }
                }
            });
        });
        ui.separator();
        egui::CollapsingHeader::new(RichText::new("EFFECT GUIDE").strong()).show(ui, |ui| {
            for (label, detail) in EFFECT_NOTES {
                ui.label(RichText::new(label).color(TEXT));
                ui.label(RichText::new(detail).small().color(DETAIL));
                ui.add_space(4.0);
            }
        });
        egui::CollapsingHeader::new(RichText::new("JOY-EDITOR").strong()).show(ui, |ui| {
            ui.label(
                RichText::new(
                    "This procedural test scene is configured here. Launch the separate editor with npm run dev:editor.",
                )
                .small()
                .color(DETAIL),
            );
        });
        egui::CollapsingHeader::new(RichText::new("Capabilities").strong()).show(ui, |ui| {
            ui.label(RichText::new(&self.capabilities).monospace().size(11.0).color(DETAIL));
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "CPU frame encoding and frame cadence are measured separately. GPU execution time is unavailable in \
                 this example.",
            )
            .small()
            .color(DETAIL),
        );
    }

    /// Title and subtitle over the top-left of the viewport.
    fn header(context: &egui::Context) {
        egui::Area::new(Id::new("scene-effects-header"))
            .anchor(Align2::LEFT_TOP, OVERLAY_MARGIN)
            .interactable(false)
            .show(context, |ui| {
                ui.label(RichText::new("JOY ENGINE / RENDERING LAB").size(11.0).color(MUTED));
                ui.label(RichText::new("LIGHT / SPACE").font(FontId::proportional(40.0)).color(TEXT));
                ui.label(RichText::new("A reproducible room for scene effects.").size(13.0).color(MUTED));
            });
    }

    /// Controls hint and statistics over the bottom-left of the viewport.
    fn footer(&self, context: &egui::Context, layout: Layout) {
        let offset = UiVec2::new(OVERLAY_MARGIN.x, -OVERLAY_MARGIN.y - layout.bottom);
        egui::Area::new(Id::new("scene-effects-footer")).anchor(Align2::LEFT_BOTTOM, offset).interactable(false).show(
            context,
            |ui| {
                ui.label(RichText::new("Drag to orbit · scroll to zoom").size(13.0).color(MUTED));
                // Statistics keep their lines, as the page's preformatted block does.
                ui.add(egui::Label::new(RichText::new(&self.stats).monospace().size(11.0).color(TEXT)).extend());
            },
        );
    }
}

/// The capture and test API: direct control over effects, the shadow fixture, and lights, plus copied snapshots.
/// Headless tests in `tests/integration/` drive the lab through it.
#[allow(
    dead_code,
    reason = "the windowed lab drives these through its interface; integration tests call them directly"
)]
impl SceneEffectsExample {
    /// The preview controls; changes reach the renderer at the next render phase.
    pub fn controls_mut(&mut self) -> &mut SceneEffectsControls {
        &mut self.controls
    }

    /// Replace the room with a shadow fixture: a high block outside the camera's view casts onto the visible floor from
    /// five lights.
    pub fn shadow_fixture(&mut self, enabled: bool) {
        self.replace_scene();
        if !enabled {
            return;
        }
        let blocker = Transform::new(Vec3::new(0.0, 20.0, 0.0), Vec3::ZERO, Vec3::new(4.0, 1.0, 4.0));
        let asset = self.room.scene.get(self.room.moving).map(|instance| instance.asset.clone());
        if let Some(asset) = asset {
            self.room.scene.insert(MeshInstance::new(asset).with_transform(blocker));
        }
        let light = |position: [f32; 3], color: f32, range: f32| PointLight {
            position: Vec3::from(position),
            color: LinearColor::rgb(color, color, color),
            range,
            casts_shadow: true,
        };
        self.room.lights = vec![
            light([0.0, 30.0, 0.0], 12.0, 70.0),
            light([-4.0, 4.0, 0.0], 0.1, 12.0),
            light([4.0, 4.0, 0.0], 0.1, 12.0),
            light([0.0, 4.0, -4.0], 0.1, 12.0),
            light([0.0, 4.0, 4.0], 0.1, 12.0),
        ];
    }

    /// Opt one light in or out of shadow casting while keeping its light.
    pub fn set_light_casting(&mut self, index: usize, enabled: bool) -> AppResult {
        let light = self.room.lights.get_mut(index).ok_or("Light index is outside the room.")?;
        light.casts_shadow = enabled;
        self.reset_history();
        Ok(())
    }

    /// Recolor one light with finite nonnegative linear RGB.
    pub fn set_light_color(&mut self, index: usize, color: [f32; 3]) -> AppResult {
        if !color.iter().all(|value| value.is_finite() && *value >= 0.0) {
            return Err("Expected finite nonnegative RGB.".into());
        }
        let light = self.room.lights.get_mut(index).ok_or("Light index is outside the room.")?;
        light.color = LinearColor::from(color);
        self.reset_history();
        Ok(())
    }

    /// Copy the lab's settings and latest statistics.
    pub fn snapshot(&self) -> LabSnapshot {
        LabSnapshot {
            effects: self.controls.configuration(),
            quality: self.controls.quality(),
            paused: self.paused,
            object_motion: self.object_motion,
            camera_motion: self.camera_motion,
            time_seconds: self.time_seconds,
            delta_seconds: self.delta_seconds,
            alternate_room: self.alternate_room,
            camera_yaw: self.camera.yaw,
            lights: self.room.lights.len(),
            casting_lights: self.room.lights.iter().filter(|light| light.casts_shadow).count(),
            meshes: self.room.scene.len(),
            diagnostics: self.diagnostics.clone(),
            frames: self.frames,
        }
    }
}

impl App for SceneEffectsExample {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        self.color_format = format!("{:?}", context.capabilities.scene_format).to_lowercase();
        Ok(())
    }

    fn input(&mut self, frame: &mut Frame<'_>) -> AppResult {
        let pointer = frame.input.pointer();
        if frame.input.action(ORBIT_ACTION).down && pointer.active {
            if let Some(previous) = self.drag {
                let delta = pointer.position - previous;
                self.camera.yaw -= delta.x * ORBIT_SENSITIVITY;
                self.camera.pitch = (self.camera.pitch + (delta.y * ORBIT_SENSITIVITY))
                    .clamp(CAMERA_PITCH_LIMITS.0, CAMERA_PITCH_LIMITS.1);
            }
            self.drag = Some(pointer.position);
        } else {
            self.drag = None;
        }
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        if !self.paused {
            self.delta_seconds = f64::from(frame.time.elapsed_seconds).min(MAXIMUM_STEP_SECONDS);
            self.time_seconds += self.delta_seconds;
        }
        let time = self.time_seconds;
        if let Some(moving) = self.room.scene.get_mut(self.room.moving) {
            let (amplitude, rate, spin) = OBJECT_MOTION;
            let (offset, yaw) = if self.object_motion {
                ((deterministic::sin(time * rate) * amplitude) as f32, (time * spin) as f32)
            } else {
                (0.0, 0.0)
            };
            moving.transform.position = MOVING_POSITION + Vec3::new(offset, 0.0, 0.0);
            moving.transform.rotation.y = yaw;
        }
        if self.camera_motion {
            let (amplitude, rate) = CAMERA_MOTION;
            self.camera.yaw = CAMERA_YAW + self.camera_offset + (deterministic::sin(time * rate) * amplitude) as f32;
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        let renderer = target.renderer();
        self.controls.apply(renderer);
        renderer.set_post_process(self.controls.with_bloom_choice(post_process_settings()));
        if std::mem::take(&mut self.reset_requested) {
            renderer.reset_history();
        }

        let inputs = FrameInputs {
            lights: &self.room.lights,
            ambient: Some(AMBIENT),
            clear_color: CLEAR_COLOR,
            ..FrameInputs::default()
        };
        let camera = self.view_camera(frame.viewport.logical_size);
        let stopwatch = Stopwatch::start();
        target.draw(&self.room.scene, &camera, &inputs)?;
        retain(&mut self.encode_samples, stopwatch.elapsed_milliseconds());
        if frame.time.elapsed_seconds > 0.0 {
            retain(&mut self.cadence_samples, f64::from(frame.time.elapsed_seconds) * 1000.0);
        }

        let renderer = target.renderer();
        self.diagnostics = renderer.diagnostics().clone();
        self.physical_size = renderer.physical_size();
        self.frames += 1;
        if self.frames % STATS_INTERVAL_FRAMES == 1 {
            self.capabilities = format!("{:#?}", renderer.scene_effects_capabilities());
            self.refresh_stats();
        }
        frame.dev_tools.set(
            "point shadows",
            format!("{} lights · {} views", self.diagnostics.shadow_point_lights, self.diagnostics.shadow_views),
        );
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, frame: &mut Frame<'_>) {
        let layout = Layout::for_viewport(frame.viewport.logical_size);
        let mut commands = Vec::new();
        self.sidebar(context, layout, &mut commands);
        Self::header(context);
        self.footer(context, layout);
        for command in commands {
            self.apply(command);
        }

        // Wheel zoom belongs to the canvas, not to scrolling panels.
        let scroll = context.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0 && !context.is_pointer_over_egui() {
            let distance = self.camera.distance * (-scroll * ZOOM_SENSITIVITY).exp();
            self.camera.distance = distance.clamp(CAMERA_DISTANCE_LIMITS.0, CAMERA_DISTANCE_LIMITS.1);
        }
    }
}

/// Keep the newest samples of a bounded timing distribution.
fn retain(samples: &mut Vec<f64>, value: f64) {
    samples.push(value);
    if samples.len() > TIMING_SAMPLES {
        samples.remove(0);
    }
}

/// Mean of a timing distribution, or zero without samples.
fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() { 0.0 } else { samples.iter().sum::<f64>() / samples.len() as f64 }
}
