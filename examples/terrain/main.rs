// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Terrain: Dragon Pit, a resident landscape for the engine's roughness-adaptive indexed terrain (continuous LOD).
//!
//!     cargo run --example terrain
//!     node scripts/rust/build-web.mjs --example terrain
//!
//! Browsers prefer WebGPU and fall back to WebGL 2; add `?backend=webgl` or `?backend=webgpu` to choose. Left-drag
//! orbits around the focus and scrolling zooms. Hold the right button to fly: the pointer looks around, W/S move along
//! the view, A/D strafe, and Q/E move down and up. Command or Alt with scrolling pans. G selects a grazing view and O
//! restores the overview. CONFIG exposes the error tolerance, LOD colors and grid, a camera tour, and the stable detail
//! region. The landscape is a scene terrain instance drawn by the engine renderer like any other scene content: lit,
//! shadowed by cascades, under the procedural sky, and post-processed.
//!
//! Demo dimensions are explicit example units (X/Z extent 2048, height = source height x 1000), not physical meters.

mod terrain_camera_controls;
mod terrain_config;

use joy_engine::development::dock::{
    self,
    DockTool,
};
use joy_engine::prelude::*;
use joy_engine::rendering::terrain::{
    TerrainAsset,
    TerrainDiagnostic,
    TerrainStats,
    parse_terrain,
    sample_terrain,
};
use joy_engine::rendering::{
    RendererDiagnostics,
    TerrainInstanceId,
};

use terrain_camera_controls::{
    TerrainCameraControls,
    WheelGesture,
    actions,
};
use terrain_config::TERRAIN_CONFIG;

/// The compiled Dragon Pit heightfield, embedded so the example runs identically natively and in browsers.
const MANIFEST: &str = include_str!("assets/dragon-pit/terrain.json");
const HEIGHTS: &[u8] = include_bytes!("assets/dragon-pit/height.bin");
/// The MIT notice for the source height data, shown in CONFIG because the data ships inside the binary.
const SOURCE_LICENSE: &str = include_str!("assets/dragon-pit/LICENSE-source.txt");
const SOURCE_URL: &str = "https://github.com/trentpolack/worldcreator/blob/main/dragon_pit_01/HeightMap.exr";

/// Camera preset actions.
const GROUND_ACTION: &str = "ground";
const OVERVIEW_ACTION: &str = "overview";
/// The HUD statistics refresh on the first frame and then every this many frames.
const STATS_INTERVAL_FRAMES: u64 = 15;
/// Largest frame step the camera integrates, in seconds.
const MAXIMUM_STEP_SECONDS: f32 = 0.05;
const HELP_TEXT: &str = "Left-drag orbits · scroll zooms · hold right-click to fly: WASD move, Q/E down/up · \
                         Cmd or Alt + scroll pans · G near ground · O overview";
const EXTENT_NOTE: &str = "Demo extent 2048 × 2048; source height × 1000. Physical dimensions are unspecified.";

struct TerrainLab {
    // Resident data and the scene that draws it.
    asset: TerrainAsset,
    scene: RenderScene,
    scene_terrain: TerrainInstanceId,
    environment: EnvironmentSystem,
    lighting: Option<EnvironmentLighting>,

    // Camera and its controls.
    camera: OrbitCamera,
    controls: TerrainCameraControls,

    // CONFIG settings.
    tolerance: f32,
    diagnostic: TerrainDiagnostic,
    pin: bool,
    tour: bool,

    // HUD state.
    backend: String,
    stats: String,
    frames: u64,
}

impl TerrainLab {
    fn new() -> Result<Self, AppError> {
        let asset = parse_terrain(MANIFEST, HEIGHTS)?;
        let mut scene = RenderScene::new();
        let scene_terrain = scene.create_terrain(asset.clone());
        let environment = EnvironmentSystem::new(EnvironmentOptions {
            time_of_day: TERRAIN_CONFIG.scene_hours / 24.0,
            day_length_seconds: 0.0,
            ..Default::default()
        })?;
        let camera = OrbitCamera {
            vertical_fov_radians: TERRAIN_CONFIG.camera_controls.vertical_fov_radians,
            ..OrbitCamera::default()
        };
        let mut lab = Self {
            asset,
            scene,
            scene_terrain,
            environment,
            lighting: None,
            camera,
            controls: TerrainCameraControls::new(TERRAIN_CONFIG.camera_controls),
            tolerance: TERRAIN_CONFIG.tolerance,
            diagnostic: TerrainDiagnostic::Material,
            pin: true,
            tour: false,
            backend: String::new(),
            stats: "Loading resident terrain…".into(),
            frames: 0,
        };
        lab.overview();
        Ok(lab)
    }

    /// Frame the landscape from above, anchoring the focus just over the terrain.
    fn overview(&mut self) {
        self.controls.stop_flight();
        let target = TERRAIN_CONFIG.target;
        let ground = sample_terrain(&self.asset, f64::from(target.x), f64::from(target.z)) as f32;
        self.camera.target = Vec3::new(target.x, ground + TERRAIN_CONFIG.target_height_offset, target.z);
        self.camera.radius = TERRAIN_CONFIG.orbit_radius;
        self.camera.distance = TERRAIN_CONFIG.orbit_distance;
        self.camera.pitch = TERRAIN_CONFIG.orbit_pitch;
        self.camera.yaw = TERRAIN_CONFIG.orbit_yaw;
    }

    /// Drop to a grazing view around the current focus.
    fn ground(&mut self) {
        self.controls.stop_flight();
        self.camera.distance = TERRAIN_CONFIG.near_ground_distance;
        self.camera.pitch = TERRAIN_CONFIG.near_ground_pitch;
    }

    /// The CONFIG panel contents.
    fn config_panel(&mut self, ui: &mut egui::Ui) {
        dock::heading(ui, "Terrain");
        let (minimum, maximum, step) = TERRAIN_CONFIG.tolerance_range;
        ui.add(egui::Slider::new(&mut self.tolerance, minimum..=maximum).step_by(f64::from(step)).text("Screen error"));
        egui::ComboBox::from_label("Visualization").selected_text(diagnostic_label(self.diagnostic)).show_ui(
            ui,
            |ui| {
                for diagnostic in
                    [TerrainDiagnostic::Material, TerrainDiagnostic::LodColors, TerrainDiagnostic::GridOverlay]
                {
                    ui.selectable_value(&mut self.diagnostic, diagnostic, diagnostic_label(diagnostic));
                }
            },
        );
        ui.checkbox(&mut self.pin, "Stable detail near test target");
        ui.checkbox(&mut self.tour, "Camera tour");
        ui.horizontal(|ui| {
            if ui.button("Near ground").clicked() {
                self.ground();
            }
            if ui.button("Overview").clicked() {
                self.overview();
            }
        });

        dock::heading(ui, "Joy-Editor");
        dock::note(
            ui,
            "Height authoring uses the offline EXR compiler. Terrain editing is not integrated into Joy Editor v1.",
        );
        dock::note(ui, "Launch the separate Joy Editor from the repository root with npm run dev:editor.");

        dock::heading(ui, "Source");
        dock::note(ui, &format!("Height data: Trent Polack's World Creator export ({SOURCE_URL}), MIT License."));
        egui::CollapsingHeader::new("Source license").show(ui, |ui| {
            ui.label(egui::RichText::new(SOURCE_LICENSE).small().monospace());
        });
    }

    /// Refresh the statistics text from the renderer's latest frame.
    fn update_stats(&mut self, target: &mut RenderTarget<'_>) {
        let renderer = target.renderer();
        self.stats = match renderer.terrain_stats(self.scene_terrain) {
            Some(stats) => terrain_stats_text(&self.backend, stats, renderer.diagnostics()),
            None => format!("{} · the landscape is outside the view", self.backend),
        };
    }
}

impl App for TerrainLab {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        self.backend = context.capabilities.backend.label().to_uppercase();
        joy_engine::log::info!("Terrain Lab ready: {}; {} resident patches.", self.backend, self.asset.patches().len());
        Ok(())
    }

    fn input(&mut self, frame: &mut Frame<'_>) -> AppResult {
        // Presets stay available outside flight; text fields keep keyboard focus (the shell suspends input).
        if frame.input.pressed(GROUND_ACTION) {
            self.ground();
        }
        if frame.input.pressed(OVERVIEW_ACTION) {
            self.overview();
        }
        self.controls.input(&mut self.camera, frame.input);
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        let seconds = frame.time.elapsed_seconds.min(MAXIMUM_STEP_SECONDS);
        self.controls.update(&mut self.camera, seconds);

        // Flight owns orientation while held; the automatic tour resumes on release.
        if self.tour && !self.controls.flying() {
            self.camera.yaw += seconds * TERRAIN_CONFIG.tour_yaw_speed;
        }
        let environment = self.environment.step(f64::from(seconds))?;
        self.lighting = Some(environment_lighting(&environment, &SkyOptions::default()));
        Ok(())
    }

    fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        // Selection runs in the renderer for the frame's camera, in CSS pixels, so display density never changes LOD.
        if let Some(instance) = self.scene.terrain_mut(self.scene_terrain) {
            instance.tolerance = self.tolerance;
            instance.target_radius = if self.pin { TERRAIN_CONFIG.target_radius } else { 0.0 };
            instance.diagnostic = self.diagnostic;
        }
        // The renderer lights and tone maps in linear light, so the sRGB background is decoded first.
        let inputs = FrameInputs {
            environment: self.lighting.as_ref(),
            clear_color: TERRAIN_CONFIG.background_color.to_linear(),
            ..Default::default()
        };
        target.draw(&self.scene, &self.camera, &inputs)?;

        self.frames += 1;
        if self.frames == 1 || self.frames.is_multiple_of(STATS_INTERVAL_FRAMES) {
            self.update_stats(target);
        }
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, frame: &mut Frame<'_>) {
        // Scrolling over the viewport zooms or pans; scrolling over UI panels belongs to them.
        if !context.is_pointer_over_egui() {
            let height = frame.viewport.logical_size.y;
            for gesture in wheel_gestures(context, height) {
                self.controls.wheel(&mut self.camera, gesture, height);
            }
        }
        if self.controls.flying() {
            context.set_cursor_icon(egui::CursorIcon::None);
        }

        egui::Area::new(egui::Id::new("terrain-header")).anchor(egui::Align2::LEFT_TOP, [16.0, 16.0]).show(
            context,
            |ui| {
                ui.label(egui::RichText::new("JOY ENGINE · RENDERING EXPERIMENT").small().color(dock::MUTED));
                ui.label(egui::RichText::new("DRAGON PIT").size(28.0).strong().color(dock::TEXT));
                ui.label(
                    egui::RichText::new("Roughness-adaptive terrain · indexed grids · continuous morphing")
                        .color(dock::TEXT),
                );
            },
        );

        let width = (context.content_rect().width() - 200.0).clamp(240.0, 720.0);
        egui::Area::new(egui::Id::new("terrain-footer")).anchor(egui::Align2::LEFT_BOTTOM, [12.0, -12.0]).show(
            context,
            |ui| {
                egui::Frame::new()
                    .fill(dock::BACKGROUND.gamma_multiply(0.92))
                    .stroke(egui::Stroke::new(1.0, dock::LINE))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_max_width(width);
                        ui.label(egui::RichText::new(HELP_TEXT).small().color(dock::MUTED));
                        ui.label(egui::RichText::new(&self.stats).monospace().size(12.0).color(dock::TEXT));
                        ui.label(egui::RichText::new(EXTENT_NOTE).small().color(dock::MUTED));
                    });
            },
        );

        dock::launcher(context, DockTool::Config, egui::RichText::new("CONFIG"));
        dock::panel(context, DockTool::Config, "CONFIG", |ui| self.config_panel(ui));
    }
}

/// This frame's scroll gestures in DOM conventions (CSS pixels; positive Y scrolls down).
fn wheel_gestures(context: &egui::Context, viewport_height: f32) -> Vec<WheelGesture> {
    let line = TERRAIN_CONFIG.camera_controls.wheel_line_pixels;
    context.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| {
                let egui::Event::MouseWheel { unit, delta, modifiers, .. } = event else {
                    return None;
                };
                // egui scrolls content (positive Y moves it down); DOM wheel deltas have the opposite sign.
                let scale = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => line,
                    egui::MouseWheelUnit::Page => viewport_height,
                };
                Some(WheelGesture {
                    delta: Vec2::new(-delta.x, -delta.y) * scale,
                    control: modifiers.ctrl,
                    pan: modifiers.alt || modifiers.mac_cmd || (modifiers.command && !modifiers.ctrl),
                })
            })
            .collect()
    })
}

/// The statistics block: the terrain's selection and residency, then the frame's renderer totals.
fn terrain_stats_text(backend: &str, stats: &TerrainStats, diagnostics: &RendererDiagnostics) -> String {
    let mebibyte = 1_048_576.0;
    format!(
        "{backend} · {} patches · {} triangles ({})\nCPU prepare {:.2} ms · GPU resident {:.2} MiB · CPU resident {:.2} \
         MiB · frame upload {} B\n{} draw calls · {} scene triangles · {} shadow views · {} visible, {} culled",
        stats.patches,
        group_thousands(stats.triangles as u64),
        stats.triangle_count_kind.label(),
        stats.cpu_milliseconds,
        stats.resident_gpu_bytes as f64 / mebibyte,
        stats.resident_cpu_bytes as f64 / mebibyte,
        group_thousands(stats.frame_upload_bytes),
        group_thousands(diagnostics.draw_calls as u64),
        group_thousands(diagnostics.triangles as u64),
        diagnostics.shadow_views,
        diagnostics.visible,
        diagnostics.culled,
    )
}

/// `1234567` as `1,234,567`, like JavaScript's `toLocaleString` in English locales.
fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

fn diagnostic_label(diagnostic: TerrainDiagnostic) -> &'static str {
    match diagnostic {
        TerrainDiagnostic::Material => "Landscape",
        TerrainDiagnostic::LodColors => "LOD colors",
        TerrainDiagnostic::GridOverlay => "LOD + grid",
    }
}

fn options() -> ShellOptions {
    let input = InputMap::new()
        .action(actions::ORBIT, [InputBinding::MouseButton(0)])
        .action(actions::FLY, [InputBinding::MouseButton(2)])
        .action(actions::FORWARD, [InputBinding::key(key::KEY_W)])
        .action(actions::BACKWARD, [InputBinding::key(key::KEY_S)])
        .action(actions::LEFT, [InputBinding::key(key::KEY_A)])
        .action(actions::RIGHT, [InputBinding::key(key::KEY_D)])
        .action(actions::DOWN, [InputBinding::key(key::KEY_Q)])
        .action(actions::UP, [InputBinding::key(key::KEY_E)])
        .action(GROUND_ACTION, [InputBinding::key(key::KEY_G)])
        .action(OVERVIEW_ACTION, [InputBinding::key(key::KEY_O)]);

    // Shadows cover the whole 2048-unit landscape; distances are world units.
    let shadows = ShadowOptions { shadow_distance: 2500.0, normal_bias: 0.5, ..ShadowOptions::default() };
    let renderer = RendererOptions { shadows: Some(shadows), ..RendererOptions::default() };
    // The default canvas ID matches `scripts/rust/build-web.mjs` pages built without `--canvas`.
    ShellOptions { title: "Joy Terrain Lab".into(), input, renderer, ..Default::default() }
}

fn start() {
    let result =
        TerrainLab::new().map_err(|error| ShellError::App(error.to_string())).and_then(|app| run(options(), app));
    if let Err(error) = result {
        joy_engine::log::error!("Terrain startup failed: {error}");
    }
}

joy_engine::web_main!(start);

fn main() {
    // Browsers start through `web_main!` instead.
    #[cfg(not(target_arch = "wasm32"))]
    start();
}
