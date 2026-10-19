// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM building shell: one FORM source generates cottage, warehouse, and apartment exteriors from parameter
//! snapshots, drawn by the engine renderer with their PBR materials.
//!
//! The source parses once; each change evaluates a complete parameter snapshot and replaces the drawn building only
//! after the new asset is ready, so a failed evaluation leaves the previous building visible. Baked mode
//! (`?mode=baked`, or `--mode=baked` natively) draws the supplied cottage record without parsing or evaluating FORM.
//! Recipes and baked meshes save as downloads in browsers and into the working directory natively.

use joy_engine::development::dock;
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
use joy_engine::form::{
    CompiledScript,
    GeometryData,
};
use joy_engine::prelude::*;
use joy_engine::rendering::ShiftedCamera;
use joy_engine::runtime::{
    Stopwatch,
    launch_parameter,
    save_text_file,
};
use joy_engine::serde_json::{
    self,
    Map,
    Value,
    json,
};

use super::asset::{
    asset_from_evaluation,
    geometry_from_mesh_data,
    mesh_data_from_evaluation,
};
use super::presentation::populate_building_scene;

/// The canonical building source and its presets.
pub const SOURCE: &str = include_str!("../assets/building.form");
const PRESETS: &str = include_str!("../assets/presets.json");
/// The cottage, evaluated and baked by `npm run bake`.
pub const BAKED_COTTAGE: &str = include_str!("../assets/cottage.mesh.json");

/// Input action held to orbit (primary mouse button or a touch).
pub const ORBIT_ACTION: &str = "orbit";
/// Launch parameter and value that select baked mode.
const MODE_PARAMETER: &str = "mode";
const MODE_BAKED: &str = "baked";
/// The preset shown at startup.
pub const START_PRESET: &str = "cottage";
/// Project name written into saved recipes, and the saved file names.
const RECIPE_NAME: &str = "Building shell";
const RECIPE_FILE: &str = "building.formlab";
const BAKE_FILE: &str = "building.mesh.json";

/// Camera framing: heading and elevation (radians), clips, orbit limits, zoom limits, and the fitted distance as a
/// multiple of the building's largest extent.
const CAMERA_YAW: f32 = 0.65;
const CAMERA_PITCH: f32 = 0.35;
const CAMERA_CLIPS: (f32, f32) = (0.2, 300.0);
const CAMERA_PITCH_LIMITS: (f32, f32) = (0.05, 1.3);
const CAMERA_DISTANCE_LIMITS: (f32, f32) = (4.0, 150.0);
const FIT_DISTANCE: f32 = 2.3;
const ORBIT_SENSITIVITY: f32 = 0.004;
const ZOOM_SENSITIVITY: f32 = 0.001;

/// Lighting: strong ambient, a dark blue background, and one warm shadowed point light above the front corner.
const AMBIENT: f32 = 0.65;
const CLEAR_COLOR: LinearColor = LinearColor::rgb(0.065, 0.105, 0.15);
const LIGHT: PointLight = PointLight {
    position: Vec3::new(8.0, 20.0, 14.0),
    color: LinearColor::rgb(4.0, 3.6, 3.1),
    range: 60.0,
    casts_shadow: true,
};

/// Render timing samples kept for the statistics.
const TIMING_SAMPLES: usize = 120;

/// Sidebar width in logical pixels, and on viewports at most [`NARROW_VIEWPORT`] wide.
const SIDEBAR_WIDTH: f32 = 330.0;
const SIDEBAR_WIDTH_NARROW: f32 = 260.0;
const NARROW_VIEWPORT: f32 = 760.0;

// The example's palette.
const SIDEBAR_FILL: Color32 = Color32::from_rgb(0x17, 0x24, 0x31);
const TITLE: Color32 = Color32::from_rgb(0xf9, 0xb2, 0x5e);
const TEXT: Color32 = Color32::from_rgb(0xe4, 0xed, 0xf4);
const MUTED: Color32 = Color32::from_rgb(0xb0, 0xc5, 0xd4);
const INPUT: Color32 = Color32::from_rgb(0x25, 0x3c, 0x4d);
const INPUT_LINE: Color32 = Color32::from_rgb(0x57, 0x75, 0x8a);
const FAILURE: Color32 = Color32::from_rgb(0xff, 0xad, 0x96);

/// Where the building comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingMode {
    /// Parsed FORM source evaluated from parameter snapshots.
    Editable,
    /// The supplied baked cottage record; nothing is evaluated.
    Baked,
}

/// One numeric FORM parameter exposed as a control, with its declared bounds.
#[derive(Clone, Debug, PartialEq)]
struct NumericParameter {
    name: String,
    value: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
}

/// A user request from the sidebar, applied after drawing it.
#[derive(Clone, Debug, PartialEq)]
enum BuildingCommand {
    Preset(String),
    Parameter(String, f64),
    Frame,
    SaveRecipe,
    Bake,
}

/// The example: owns the parsed program, parameter snapshot, scene, camera, and timing samples.
pub struct BuildingExample {
    // Content.
    mode: BuildingMode,
    program: Option<CompiledScript>,
    presets: Map<String, Value>,
    preset: String,
    overrides: Map<String, Value>,
    parameters: Vec<NumericParameter>,
    geometry: Option<GeometryData>,
    asset: Option<MeshAsset>,

    // Presentation.
    scene: RenderScene,
    camera: OrbitCamera,
    drag: Option<Vec2>,
    reset_history: bool,

    // CPU timings in milliseconds; GPU execution is not measured.
    evaluations: u32,
    evaluation_milliseconds: f64,
    preparation_milliseconds: f64,
    render_samples: Vec<f64>,
    backend: &'static str,

    // Messages for the sidebar.
    failure: String,
    notice: String,
}

impl BuildingExample {
    /// The example in `mode`, showing the cottage. Fails when the bundled source, presets, or baked record are invalid.
    pub fn new(mode: BuildingMode) -> AppResult<Self> {
        let presets: Map<String, Value> = serde_json::from_str(PRESETS)?;
        let program = match mode {
            BuildingMode::Editable => Some(CompiledScript::parse(SOURCE)?),
            BuildingMode::Baked => None,
        };
        let parameters = match &program {
            Some(program) => scalar_parameters(&program.evaluate(&Value::Object(Map::new()), None)?),
            None => Vec::new(),
        };
        let camera = OrbitCamera {
            yaw: CAMERA_YAW,
            pitch: CAMERA_PITCH,
            near_clip: Some(CAMERA_CLIPS.0),
            far_clip: Some(CAMERA_CLIPS.1),
            ..OrbitCamera::default()
        };
        let mut example = Self {
            mode,
            program,
            presets,
            preset: START_PRESET.to_owned(),
            overrides: Map::new(),
            parameters,
            geometry: None,
            asset: None,
            scene: RenderScene::new(),
            camera,
            drag: None,
            reset_history: false,
            evaluations: 0,
            evaluation_milliseconds: 0.0,
            preparation_milliseconds: 0.0,
            render_samples: Vec::new(),
            backend: "unknown",
            failure: String::new(),
            notice: String::new(),
        };
        match mode {
            BuildingMode::Editable => example.set_preset(START_PRESET)?,
            BuildingMode::Baked => {
                let data = serde_json::from_str(BAKED_COTTAGE)?;
                let geometry = geometry_from_mesh_data(&data);
                let asset = asset_from_evaluation(&geometry)?;
                example.replace_asset(asset, geometry);
                example.fit_camera();
            }
        }
        Ok(example)
    }

    /// The mode the launch parameters select: baked with `mode=baked`, editable otherwise.
    pub fn launch_mode() -> BuildingMode {
        if launch_parameter(MODE_PARAMETER).as_deref() == Some(MODE_BAKED) {
            BuildingMode::Baked
        } else {
            BuildingMode::Editable
        }
    }

    /// Evaluate a named preset and frame it. Baked mode ignores presets.
    pub fn set_preset(&mut self, name: &str) -> AppResult {
        if self.mode == BuildingMode::Baked {
            return Ok(());
        }
        let Some(Value::Object(overrides)) = self.presets.get(name).cloned() else {
            return Err(format!("Unknown preset: {name}").into());
        };
        self.regenerate(overrides)?;
        self.preset = name.to_owned();
        self.fit_camera();
        Ok(())
    }

    /// Evaluate a complete parameter snapshot; the drawn building changes only after the new asset is ready.
    ///
    /// On failure the previous building, parameters, and timings stay as they were and the located diagnostic is
    /// returned.
    pub fn regenerate(&mut self, overrides: Map<String, Value>) -> AppResult {
        let Some(program) = &self.program else {
            return Ok(());
        };
        let stopwatch = Stopwatch::start();
        let geometry = program.evaluate(&Value::Object(overrides.clone()), None)?;
        let evaluated = stopwatch.elapsed_milliseconds();
        let asset = asset_from_evaluation(&geometry)?;
        for parameter in &mut self.parameters {
            let value = geometry
                .parameters
                .iter()
                .find(|candidate| candidate.get("name").and_then(Value::as_str) == Some(&parameter.name))
                .and_then(|candidate| candidate.get("value"))
                .and_then(Value::as_f64);
            if let Some(value) = value {
                parameter.value = value;
            }
        }
        self.replace_asset(asset, geometry);
        self.overrides = overrides;
        self.evaluation_milliseconds = evaluated;
        self.preparation_milliseconds = stopwatch.elapsed_milliseconds() - evaluated;
        self.evaluations += 1;
        self.failure.clear();
        Ok(())
    }

    /// Point the camera at the building's middle from 2.3 times its largest extent.
    pub fn fit_camera(&mut self) {
        let Some(asset) = &self.asset else {
            return;
        };
        let bounds = asset.bounds();
        self.camera.target = Vec3::new(0.0, (bounds.min.y + bounds.max.y) * 0.5, 0.0);
        self.camera.distance = (bounds.max - bounds.min).max_element() * FIT_DISTANCE;
    }

    /// The current editable recipe: a FORM LAB document with the source and parameter snapshot.
    pub fn recipe(&self) -> Value {
        json!({"version": 1, "name": RECIPE_NAME, "source": SOURCE, "overrides": self.overrides})
    }

    /// The current building's baked mesh record, or `None` before the first evaluation in editable mode.
    pub fn baked_mesh(&self) -> Option<Value> {
        let geometry = self.geometry.as_ref()?;
        serde_json::to_value(mesh_data_from_evaluation(geometry)).ok()
    }

    /// Copied diagnostics: mode, evaluation counts and CPU timings in milliseconds, geometry sizes, scene instances,
    /// bounds in meters, and render encoding timings.
    pub fn snapshot(&self) -> Value {
        let geometry = self.geometry.as_ref();
        let bounds = self.asset.as_ref().map(MeshAsset::bounds);
        let mut ordered = self.render_samples.clone();
        ordered.sort_by(f64::total_cmp);
        let p95 = ordered.get(((ordered.len().max(1) - 1) as f64 * 0.95) as usize).copied().unwrap_or(0.0);
        json!({
            "backend": self.backend,
            "mode": if self.mode == BuildingMode::Baked { "baked" } else { "editable" },
            "evaluations": self.evaluations,
            "evaluationMilliseconds": round(self.evaluation_milliseconds),
            "preparationMilliseconds": round(self.preparation_milliseconds),
            "vertices": geometry.map_or(0, |geometry| geometry.positions.len() / 3),
            "triangles": geometry.map_or(0, |geometry| geometry.triangles.len() / 3),
            "materialCount": geometry.map_or(0, |geometry| geometry.materials.len()),
            "instances": self.scene.len(),
            "bounds": bounds.map(|bounds| json!({
                "min": bounds.min.to_array().map(|value| round(f64::from(value))),
                "max": bounds.max.to_array().map(|value| round(f64::from(value))),
            })),
            "renderSubmission": {
                "samples": self.render_samples.len(),
                "meanMilliseconds": round(mean(&self.render_samples)),
                "p95Milliseconds": round(p95),
                "note": "CPU frame encoding; GPU execution is not measured.",
            },
        })
    }

    fn replace_asset(&mut self, asset: MeshAsset, geometry: GeometryData) {
        // The scene holds asset handles, not GPU meshes; clearing drops the previous generation's last reference.
        populate_building_scene(&mut self.scene, asset.clone());
        self.asset = Some(asset);
        self.geometry = Some(geometry);
        self.render_samples.clear();
        self.reset_history = true;
    }

    fn apply(&mut self, command: BuildingCommand) {
        let result = match command {
            BuildingCommand::Preset(name) => self.set_preset(&name),
            BuildingCommand::Parameter(name, value) => {
                let mut overrides = self.overrides.clone();
                overrides.insert(name, Value::from(value));
                self.regenerate(overrides)
            }
            BuildingCommand::Frame => {
                self.fit_camera();
                Ok(())
            }
            BuildingCommand::SaveRecipe => self.save(RECIPE_FILE, Some(self.recipe())),
            BuildingCommand::Bake => self.save(BAKE_FILE, self.baked_mesh()),
        };
        if let Err(error) = result {
            self.failure = error.to_string();
        }
    }

    fn save(&mut self, file_name: &str, value: Option<Value>) -> AppResult {
        let value = value.ok_or("Nothing to save yet.")?;
        let text = serde_json::to_string_pretty(&value)?;
        self.notice = format!("Saved {}", save_text_file(file_name, &text)?);
        Ok(())
    }

    /// The orbit camera with its image centered beside the sidebar.
    fn view_camera(&self, viewport: Vec2) -> ShiftedCamera<'_, OrbitCamera> {
        ShiftedCamera::between_insets(&self.camera, viewport.x, 0.0, sidebar_width(viewport.x))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Interface
    // -----------------------------------------------------------------------------------------------------------------

    fn sidebar(&self, context: &egui::Context, commands: &mut Vec<BuildingCommand>) {
        let screen = context.content_rect();
        let width = sidebar_width(screen.width());
        let margin: i8 = if width == SIDEBAR_WIDTH { 24 } else { 14 };
        let reserved = dock::reserved_height(context);
        egui::Area::new(Id::new("form-building-sidebar"))
            .anchor(Align2::RIGHT_TOP, UiVec2::ZERO)
            .constrain(false)
            .show(context, |ui| {
                egui::Frame::new().fill(SIDEBAR_FILL).inner_margin(Margin::same(margin)).show(ui, |ui| {
                    let inner = UiVec2::new(width, screen.height()) - UiVec2::splat(f32::from(margin) * 2.0);
                    ui.set_width(inner.x);
                    ui.set_height(inner.y);
                    style(ui);
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        self.sidebar_contents(ui, commands);
                        ui.add_space(reserved);
                    });
                });
            });
    }

    fn sidebar_contents(&self, ui: &mut egui::Ui, commands: &mut Vec<BuildingCommand>) {
        let editable = self.mode == BuildingMode::Editable;
        ui.label(RichText::new("FORM / BUILDING SHELL").font(FontId::proportional(19.0)).strong().color(TITLE));
        ui.label(RichText::new("One source. Three building types. Drag to orbit; scroll to zoom.").color(MUTED));
        ui.add_space(6.0);

        ui.add_enabled_ui(editable, |ui| {
            row(ui, "Preset", |ui| {
                egui::ComboBox::from_id_salt("form-building-preset").selected_text(&self.preset).show_ui(ui, |ui| {
                    for name in self.presets.keys() {
                        if ui.selectable_label(*name == self.preset, name).clicked() {
                            commands.push(BuildingCommand::Preset(name.clone()));
                        }
                    }
                });
            });
            for parameter in &self.parameters {
                row(ui, &parameter.name.replace('_', " "), |ui| {
                    let mut value = parameter.value;
                    let field = egui::DragValue::new(&mut value)
                        .range(parameter.minimum..=parameter.maximum)
                        .speed(parameter.step.max(0.01))
                        .max_decimals(4);
                    if ui.add_sized([96.0, 22.0], field).changed() {
                        commands.push(BuildingCommand::Parameter(parameter.name.clone(), value));
                    }
                });
            }
        });
        ui.label(RichText::new("Roof shape: 0 flat, 1 gable. Windows and entrances are facade panels.").color(MUTED));
        ui.add_space(4.0);

        let full = |ui: &mut egui::Ui, label: &str, enabled: bool| {
            ui.add_enabled(enabled, egui::Button::new(label).min_size(UiVec2::new(ui.available_width(), 28.0)))
                .clicked()
        };
        if full(ui, "Frame view", true) {
            commands.push(BuildingCommand::Frame);
        }
        if full(ui, "Save editable recipe", editable) {
            commands.push(BuildingCommand::SaveRecipe);
        }
        if full(ui, "Bake mesh JSON", true) {
            commands.push(BuildingCommand::Bake);
        }
        if !self.notice.is_empty() {
            ui.label(RichText::new(&self.notice).small().color(MUTED));
        }
        let stats = serde_json::to_string_pretty(&self.snapshot()).unwrap_or_default();
        ui.label(RichText::new(stats).monospace().size(12.0).color(TEXT));
        if !self.failure.is_empty() {
            ui.label(RichText::new(&self.failure).color(FAILURE));
        }
    }
}

impl App for BuildingExample {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        self.backend = context.capabilities.backend.label();
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

    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        if std::mem::take(&mut self.reset_history) {
            target.renderer().reset_history();
        }
        let lights = [LIGHT];
        let inputs =
            FrameInputs { lights: &lights, ambient: Some(AMBIENT), clear_color: CLEAR_COLOR, ..FrameInputs::default() };
        let camera = self.view_camera(frame.viewport.logical_size);
        let stopwatch = Stopwatch::start();
        target.draw(&self.scene, &camera, &inputs)?;
        self.render_samples.push(stopwatch.elapsed_milliseconds());
        if self.render_samples.len() > TIMING_SAMPLES {
            self.render_samples.remove(0);
        }
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        let mut commands = Vec::new();
        self.sidebar(context, &mut commands);
        for command in commands {
            self.apply(command);
        }

        // Wheel zoom belongs to the canvas, not to the scrolling sidebar.
        let scroll = context.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0 && !context.is_pointer_over_egui() {
            let distance = self.camera.distance * (-scroll * ZOOM_SENSITIVITY).exp();
            self.camera.distance = distance.clamp(CAMERA_DISTANCE_LIMITS.0, CAMERA_DISTANCE_LIMITS.1);
        }
    }
}

/// Sidebar width for a viewport width in logical pixels.
fn sidebar_width(viewport_width: f32) -> f32 {
    if viewport_width <= NARROW_VIEWPORT { SIDEBAR_WIDTH_NARROW } else { SIDEBAR_WIDTH }
}

/// Scalar parameters with bounds, read from an evaluation's descriptors.
fn scalar_parameters(geometry: &GeometryData) -> Vec<NumericParameter> {
    geometry
        .parameters
        .iter()
        .filter(|parameter| parameter.get("type").and_then(Value::as_str) == Some("scalar"))
        .filter_map(|parameter| {
            let number = |name: &str| parameter.get(name).and_then(Value::as_f64);
            Some(NumericParameter {
                name: parameter.get("name")?.as_str()?.to_owned(),
                value: number("value")?,
                minimum: number("min")?,
                maximum: number("max")?,
                step: number("step").unwrap_or(0.0),
            })
        })
        .collect()
}

/// A labeled control row with the control right-aligned.
fn row(ui: &mut egui::Ui, label: &str, add_control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add_control);
    });
}

/// The sidebar's input and button look.
fn style(ui: &mut egui::Ui) {
    let visuals = ui.visuals_mut();
    visuals.override_text_color = Some(TEXT);
    visuals.extreme_bg_color = INPUT;
    for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        widget.weak_bg_fill = INPUT;
        widget.bg_fill = INPUT;
        widget.bg_stroke = Stroke::new(1.0, INPUT_LINE);
        widget.corner_radius = 4.0.into();
    }
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, TITLE);
    ui.spacing_mut().item_spacing.y = 10.0;
}

/// Mean of a timing distribution, or zero without samples.
fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() { 0.0 } else { samples.iter().sum::<f64>() / samples.len() as f64 }
}

/// Round a displayed measurement to four decimals.
fn round(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}
