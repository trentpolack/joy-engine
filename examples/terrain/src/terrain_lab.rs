// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Terrain Lab app: Dragon Pit streamed as a paged quadtree, with synthesized detail below the authored spacing,
//! craters placed under the pointer, and physics props that land on the terrain colliders.
//!
//! Left-drag orbits around the focus and scrolling zooms. Hold the right button to fly: the pointer looks around, W/S
//! move along the view, A/D strafe, and Q/E move down and up. Command or Alt with scrolling pans. G selects a grazing
//! view and O restores the overview. C blasts a crater under the pointer, and P drops props near the focus. CONFIG
//! exposes the error tolerance, LOD colors and grid, synthesized detail, a camera tour, and the stable detail region.
//!
//! The embedded asset streams from memory with a short simulated delay. A package written by `joy-terrain-package`
//! streams instead with `--package=<folder>` natively or `?package=<url prefix>` in browsers, so a browser downloads
//! only the tiles the view and the props need.

use joy_engine::development::dock::{
    self,
    DockTool,
};
use joy_engine::prelude::*;
use joy_engine::rendering::{
    RendererDiagnostics,
    TerrainInstance,
    TerrainInstanceId,
    primitive_box,
    primitive_sphere,
};
use joy_engine::runtime::{
    AssetFetcher,
    launch_parameter,
};
use joy_engine::terrain::{
    PACKAGE_MANIFEST_FILE,
    TerrainAsset,
    TerrainDiagnostic,
    TerrainPackage,
    TerrainStats,
    parse_terrain,
    sample_terrain,
};

use super::terrain_camera_controls::{
    TerrainCameraControls,
    WheelGesture,
    actions,
};
use super::terrain_config::TERRAIN_CONFIG;
use super::terrain_world::{
    PropKind,
    TerrainWorld,
    WorldSnapshot,
    terrain_view,
};

/// The compiled Dragon Pit heightfield, embedded so the example runs identically natively and in browsers.
const MANIFEST: &str = include_str!("../assets/dragon-pit/terrain.json");
const HEIGHTS: &[u8] = include_bytes!("../assets/dragon-pit/height.bin");
/// The MIT notice for the source height data, shown in CONFIG because the data ships inside the binary.
const SOURCE_LICENSE: &str = include_str!("../assets/dragon-pit/LICENSE-source.txt");
const SOURCE_URL: &str = "https://github.com/trentpolack/worldcreator/blob/main/dragon_pit_01/HeightMap.exr";

/// Launch parameter naming a package folder (natively) or URL prefix (in browsers) to stream instead.
pub const PACKAGE_PARAMETER: &str = "package";

/// Camera preset and edit actions.
pub const GROUND_ACTION: &str = "ground";
/// Restore the overview.
pub const OVERVIEW_ACTION: &str = "overview";
/// Blast a crater under the pointer.
pub const CRATER_ACTION: &str = "crater";
/// Drop props near the focus.
pub const PROPS_ACTION: &str = "props";

/// A prop moving slower than this many world units per second counts as at rest.
pub const RESTING_SPEED: f32 = 0.25;
/// The HUD statistics refresh on the first frame and then every this many frames.
const STATS_INTERVAL_FRAMES: u64 = 15;
/// Largest frame step the camera and physics integrate, in seconds.
const MAXIMUM_STEP_SECONDS: f32 = 0.05;
const HELP_TEXT: &str = "Left-drag orbits · scroll zooms · hold right-click to fly: WASD move, Q/E down/up · \
                         Cmd or Alt + scroll pans · G near ground · O overview · C crater · P props";
const EXTENT_NOTE: &str = "Demo extent 2048 × 2048; source height × 1000. Physical dimensions are unspecified.";

/// A package manifest requested at startup and not yet arrived.
struct PendingPackage {
    fetcher: AssetFetcher,
    root: String,
}

/// One drawn prop: the body it follows and its mesh instance.
struct PropMesh {
    body: joy_engine::physics::BodyHandle,
    mesh: MeshInstanceId,
}

/// Copied lab state for the HUD and tests.
#[derive(Clone, Debug, PartialEq)]
pub struct LabSnapshot {
    /// The drawn terrain instance, once the first surface arrived.
    pub terrain: Option<TerrainInstanceId>,
    /// The camera focus in world units.
    pub focus: Vec3,
    /// The camera.
    pub camera: OrbitCamera,
    /// The terrain systems' state, once the tile source is ready.
    pub world: Option<WorldSnapshot>,
}

/// The example app. See the module documentation for controls.
pub struct TerrainLab {
    // Terrain systems and the scene that draws them.
    asset: TerrainAsset,
    world: Option<TerrainWorld>,
    pending_package: Option<PendingPackage>,
    scene: RenderScene,
    scene_terrain: Option<TerrainInstanceId>,
    prop_meshes: Vec<PropMesh>,
    environment: EnvironmentSystem,
    lighting: Option<EnvironmentLighting>,

    // Camera and its controls.
    camera: OrbitCamera,
    controls: TerrainCameraControls,
    grounded: bool,

    // CONFIG settings and pending edits.
    tolerance: f32,
    diagnostic: TerrainDiagnostic,
    detail: bool,
    pin: bool,
    tour: bool,
    crater_request: Option<Vec2>,
    props_requested: bool,

    // HUD state.
    backend: String,
    status: String,
    stats: String,
    frames: u64,
}

impl TerrainLab {
    /// The lab streaming the embedded asset, or the package named by the `package` launch parameter.
    pub fn new() -> AppResult<Self> {
        let asset = parse_terrain(MANIFEST, HEIGHTS)?;
        let detail = true;
        let pending_package = launch_parameter(PACKAGE_PARAMETER).filter(|root| !root.is_empty()).map(|root| {
            let mut fetcher = AssetFetcher::new(root.clone());
            fetcher.request(PACKAGE_MANIFEST_FILE);
            PendingPackage {
                fetcher,
                root,
            }
        });
        let world = if pending_package.is_some() { None } else { Some(TerrainWorld::embedded(&asset, detail)?) };
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
            world,
            pending_package,
            scene: RenderScene::new(),
            scene_terrain: None,
            prop_meshes: Vec::new(),
            environment,
            lighting: None,
            camera,
            controls: TerrainCameraControls::new(TERRAIN_CONFIG.camera_controls),
            grounded: false,
            tolerance: TERRAIN_CONFIG.tolerance,
            diagnostic: TerrainDiagnostic::Material,
            detail,
            pin: true,
            tour: false,
            crater_request: None,
            props_requested: false,
            backend: String::new(),
            status: "Streaming the root tiles…".into(),
            stats: String::new(),
            frames: 0,
        };
        lab.overview();
        Ok(lab)
    }

    /// Copy the state the HUD and tests read.
    pub fn snapshot(&self) -> LabSnapshot {
        LabSnapshot {
            terrain: self.scene_terrain,
            focus: self.camera.target,
            camera: self.camera,
            world: self.world.as_ref().map(|world| world.snapshot(self.camera.target)),
        }
    }

    /// Frame the landscape from above, anchoring the focus just over the terrain.
    pub fn overview(&mut self) {
        self.controls.stop_flight();
        let target = TERRAIN_CONFIG.target;
        let ground = self.ground_height(target);
        self.camera.target = Vec3::new(target.x, ground + TERRAIN_CONFIG.target_height_offset, target.z);
        self.camera.radius = TERRAIN_CONFIG.orbit_radius;
        self.camera.distance = TERRAIN_CONFIG.orbit_distance;
        self.camera.pitch = TERRAIN_CONFIG.orbit_pitch;
        self.camera.yaw = TERRAIN_CONFIG.orbit_yaw;
    }

    /// Drop to a grazing view around the current focus.
    pub fn ground(&mut self) {
        self.controls.stop_flight();
        self.camera.distance = TERRAIN_CONFIG.near_ground_distance;
        self.camera.pitch = TERRAIN_CONFIG.near_ground_pitch;
    }

    /// Terrain height under a point: the streamed surface when resident, else the embedded asset (package mode falls
    /// back to the configured focus height).
    fn ground_height(&self, point: Vec3) -> f32 {
        if let Some(height) = self.world.as_ref().and_then(|world| world.ground_height(point)) {
            return height as f32;
        }
        if self.pending_package.is_none() {
            return sample_terrain(&self.asset, f64::from(point.x), f64::from(point.z)) as f32;
        }
        TERRAIN_CONFIG.target.y
    }

    /// Finish loading a package manifest; a failure falls back to the embedded asset.
    fn poll_package(&mut self) -> AppResult {
        let Some(pending) = &mut self.pending_package else {
            return Ok(());
        };
        let Some((_, result)) = pending.fetcher.poll().into_iter().next() else {
            return Ok(());
        };
        let root = pending.root.clone();
        self.pending_package = None;
        let package = result
            .and_then(|bytes| String::from_utf8(bytes).map_err(|error| error.to_string()))
            .and_then(|text| TerrainPackage::parse(&text).map_err(|error| error.to_string()));
        match package {
            Ok(package) => {
                self.status = format!("Streaming package {root}…");
                self.world = Some(TerrainWorld::package(package, root, self.detail)?);
            }
            Err(message) => {
                joy_engine::log::warn!("Terrain package {root} unavailable ({message}); streaming the embedded asset.");
                self.status = format!("Package unavailable ({message}); streaming the embedded asset.");
                self.world = Some(TerrainWorld::embedded(&self.asset, self.detail)?);
            }
        }
        Ok(())
    }

    /// Stream for this frame's camera and publish any new snapshot into the scene.
    fn stream(&mut self, logical_height: f32) -> AppResult {
        let Some(world) = &mut self.world else {
            return Ok(());
        };
        world.set_detail(self.detail)?;
        let eye = self.camera.basis().eye;
        let radius = if self.pin { TERRAIN_CONFIG.target_radius } else { 0.0 };
        let view = terrain_view(eye, self.camera.target, logical_height, self.tolerance, radius);
        let Some(surface) = world.update_streaming(view, self.camera.target)? else {
            return Ok(());
        };
        match self.scene_terrain.and_then(|id| self.scene.terrain_mut(id)) {
            Some(instance) => instance.surface = surface,
            None => self.scene_terrain = Some(self.scene.insert_terrain(TerrainInstance::new(surface))),
        }

        // Once real terrain arrives, rest the overview focus on it.
        if !self.grounded {
            self.grounded = true;
            self.status.clear();
            self.overview();
        }
        Ok(())
    }

    /// Add meshes for new props, follow every prop's pose, and remove meshes whose props left.
    fn sync_props(&mut self) {
        let Some(world) = &self.world else {
            return;
        };
        let config = TERRAIN_CONFIG.props;
        let live: Vec<_> = world.props().to_vec();
        self.prop_meshes.retain(|drawn| {
            let keep = live.iter().any(|prop| prop.body == drawn.body);
            if !keep {
                self.scene.remove(drawn.mesh);
            }
            keep
        });
        for prop in &live {
            if self.prop_meshes.iter().any(|drawn| drawn.body == prop.body) {
                continue;
            }
            let (asset, tint, size) = match prop.kind {
                PropKind::Crate => (primitive_box(), config.crate_color, config.crate_half_extent * 2.0),
                PropKind::Ball => (primitive_sphere(), config.ball_color, config.ball_radius * 2.0),
            };
            let mut instance = MeshInstance::new(asset);
            instance.tint = tint;
            instance.transform.scale = Vec3::splat(size);
            self.prop_meshes.push(PropMesh {
                body: prop.body,
                mesh: self.scene.insert(instance),
            });
        }
        for drawn in &self.prop_meshes {
            let pose = live.iter().find(|prop| prop.body == drawn.body).and_then(|prop| world.prop_transform(prop));
            if let (Some(pose), Some(instance)) = (pose, self.scene.get_mut(drawn.mesh)) {
                instance.transform.position = pose.position;
                instance.transform.rotation = pose.rotation;
            }
        }
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
        ui.checkbox(&mut self.detail, "Synthesized detail below the authored spacing");
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

        dock::heading(ui, "Edits and physics");
        ui.horizontal(|ui| {
            if ui.button("Crater at focus").clicked() {
                self.crater_request = Some(Vec2::splat(-1.0));
            }
            if ui.button("Drop props").clicked() {
                self.props_requested = true;
            }
        });
        dock::note(ui, "C blasts a crater under the pointer; P drops SDF crates and spheres near the focus.");

        dock::heading(ui, "Joy-Editor");
        dock::note(
            ui,
            "Package a heightfield for streaming with joy-terrain-package. Terrain editing is not integrated into Joy Editor.",
        );
        dock::note(ui, "Launch the separate Joy Editor from the repository root with npm run dev:editor.");

        dock::heading(ui, "Source");
        dock::note(ui, &format!("Height data: Trent Polack's World Creator export ({SOURCE_URL}), MIT License."));
        egui::CollapsingHeader::new("Source license").show(ui, |ui| {
            ui.label(egui::RichText::new(SOURCE_LICENSE).small().monospace());
        });
    }

    /// Refresh the statistics text from the renderer's latest frame, the streaming report, and the physics state.
    fn update_stats(&mut self, target: &mut RenderTarget<'_>) {
        let snapshot = self.snapshot();
        let renderer = target.renderer();
        let terrain = snapshot.terrain.and_then(|id| renderer.terrain_stats(id));
        self.stats = match (terrain, &snapshot.world) {
            (Some(stats), Some(world)) => {
                let streaming = &world.streaming;
                let ground = world.focus_ground.map(|height| format!("{height:.2}")).unwrap_or_else(|| "–".into());
                let crates = world.props.iter().filter(|prop| prop.kind == PropKind::Crate).count();
                let resting = world.props.iter().filter(|prop| prop.at_rest(RESTING_SPEED)).count();
                format!(
                    "{}\nstreaming · {} resident · {} pending · {} synthesized · {} waiting · detail {}\nphysics · {} \
                     terrain colliders · {} SDF crates and {} spheres ({resting} at rest) · {} craters · gameplay \
                     ground under focus {ground}",
                    terrain_stats_text(&self.backend, stats, renderer.diagnostics()),
                    streaming.resident_tiles,
                    streaming.pending_loads,
                    streaming.synthesized,
                    streaming.waiting,
                    if world.detail { "on" } else { "off" },
                    world.colliders,
                    crates,
                    world.props.len() - crates,
                    world.craters.len(),
                )
            }
            (_, Some(world)) if world.ready && self.status.is_empty() => {
                format!("{} · the landscape is outside the view", self.backend)
            }
            _ if self.status.is_empty() => format!("{} · streaming the root tiles…", self.backend),
            _ => format!("{} · {}", self.backend, self.status),
        };
    }
}

impl App for TerrainLab {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        self.backend = context.capabilities.backend.label().to_uppercase();
        joy_engine::log::info!("Terrain Lab ready: {}.", self.backend);
        Ok(())
    }

    fn input(&mut self, frame: &mut Frame<'_>) -> AppResult {
        // Presets and edits stay available outside flight; text fields keep keyboard focus (the shell suspends input).
        if frame.input.pressed(GROUND_ACTION) {
            self.ground();
        }
        if frame.input.pressed(OVERVIEW_ACTION) {
            self.overview();
        }
        if frame.input.pressed(CRATER_ACTION) {
            let pointer = frame.input.pointer();
            let center = frame.viewport.logical_size * 0.5;
            self.crater_request = Some(if pointer.active { pointer.position } else { center });
        }
        if frame.input.pressed(PROPS_ACTION) {
            self.props_requested = true;
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

        // Stream for the camera, then drop requested props and step physics against the current colliders.
        self.poll_package()?;
        self.stream(frame.viewport.logical_size.y)?;
        let focus = self.camera.target;
        if let Some(world) = &mut self.world {
            if std::mem::take(&mut self.props_requested) && world.surface().is_some() {
                world.drop_props(focus)?;
            }
            world.step_physics(seconds, focus)?;
        }
        self.sync_props();
        Ok(())
    }

    fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        // A crater request picks the surface under the pointer (or the focus, for the CONFIG button).
        if let (Some(point), Some(world)) = (self.crater_request.take(), &mut self.world) {
            let placed = if point.x < 0.0 {
                let eye = self.camera.basis().eye;
                world.place_crater(eye, self.camera.target - eye)?
            } else {
                match target.renderer().screen_ray(&self.camera, point) {
                    Some(ray) => world.place_crater(ray.origin, ray.direction)?,
                    None => None,
                }
            };
            if placed.is_none() {
                self.status = "No terrain under the pointer for a crater.".into();
            }
        }

        // Selection runs in the renderer for the frame's camera, in CSS pixels, so display density never changes LOD.
        if let Some(instance) = self.scene_terrain.and_then(|id| self.scene.terrain_mut(id)) {
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
                    egui::RichText::new("Streamed quadtree terrain · synthesized detail · craters · terrain physics")
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

/// The example's input bindings.
pub fn input_map() -> InputMap {
    InputMap::new()
        .action(actions::ORBIT, [InputBinding::MouseButton(0)])
        .action(actions::FLY, [InputBinding::MouseButton(2)])
        .action(actions::FORWARD, [InputBinding::key(key::KEY_W)])
        .action(actions::BACKWARD, [InputBinding::key(key::KEY_S)])
        .action(actions::LEFT, [InputBinding::key(key::KEY_A)])
        .action(actions::RIGHT, [InputBinding::key(key::KEY_D)])
        .action(actions::DOWN, [InputBinding::key(key::KEY_Q)])
        .action(actions::UP, [InputBinding::key(key::KEY_E)])
        .action(GROUND_ACTION, [InputBinding::key(key::KEY_G)])
        .action(OVERVIEW_ACTION, [InputBinding::key(key::KEY_O)])
        .action(CRATER_ACTION, [InputBinding::key(key::KEY_C)])
        .action(PROPS_ACTION, [InputBinding::key(key::KEY_P)])
}

/// Renderer settings: shadows cover the whole 2048-unit landscape (world units), under the atmosphere.
pub fn renderer_options() -> RendererOptions {
    let config = TERRAIN_CONFIG.shadows;
    let shadows = ShadowOptions {
        shadow_distance: config.distance,
        cascade_split_lambda: config.cascade_split_lambda,
        directional_map_size: Some(config.map_size),
        normal_bias: config.normal_bias,
        ..ShadowOptions::default()
    };
    RendererOptions {
        shadows: Some(shadows),
        atmosphere: true,
        ..RendererOptions::default()
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
                let egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } = event
                else {
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
        "{backend} · {} of {} selected tiles in view, {} shadow tile draws (finest level {}, {:.1} px tolerance) · {} \
         triangles of {}\nCPU prepare {:.2} ms · GPU resident {:.2} MiB in {} pool layers · CPU resident {:.2} MiB · {} \
         tiles uploaded · frame upload {} B\n{} draw calls · {} scene triangles · {} shadow views · {} visible, {} culled",
        stats.visible_patches,
        stats.patches,
        stats.shadow_patches,
        stats.finest_level,
        stats.tolerance,
        group_thousands(stats.triangles as u64),
        group_thousands(stats.max_triangles as u64),
        stats.cpu_milliseconds,
        stats.resident_gpu_bytes as f64 / mebibyte,
        stats.pool_layers,
        stats.resident_cpu_bytes as f64 / mebibyte,
        stats.uploaded_tiles,
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
