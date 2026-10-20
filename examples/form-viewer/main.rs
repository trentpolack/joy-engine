// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM viewer: compiles the engine's authored FORM assets and draws them with the engine renderer under the procedural
//! sky, with live parameter, shading, and time-of-day controls.
//!
//!     cargo run -p joy-engine --example form-viewer
//!
//! Drag with the left mouse button to orbit. FORM itself has no renderer; this is the same path games and the editor
//! use, so what you see here is what a level shows.

use joy_engine::prelude::*;
use joy_engine::serde_json::{
    Map,
    Value,
};

/// Built-in assets, compiled from source at startup so the viewer runs identically natively and in browsers.
const ASSETS: [(&str, &str); 6] = [
    ("assets/form/structures/building.form", include_str!("../../assets/form/structures/building.form")),
    ("assets/form/structures/cottage.formlab", include_str!("../../assets/form/structures/cottage.formlab")),
    ("assets/form/structures/warehouse.formlab", include_str!("../../assets/form/structures/warehouse.formlab")),
    ("assets/form/structures/apartments.formlab", include_str!("../../assets/form/structures/apartments.formlab")),
    ("assets/examples/parametric-bridge.form", include_str!("../../assets/examples/parametric-bridge.form")),
    ("assets/examples/satellite-garden.form", include_str!("../../assets/examples/satellite-garden.form")),
];

/// One scalar parameter exposed as a slider.
struct Parameter {
    name: String,
    value: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
}

struct FormViewer {
    library: FormAssetLibrary,
    scene: RenderScene,
    camera: OrbitCamera,
    environment: EnvironmentSystem,
    lighting: Option<EnvironmentLighting>,
    selected: usize,
    shading: ShadingModel<'static>,
    parameters: Vec<Parameter>,
    instance: Option<MeshInstanceId>,
    error: Option<String>,
    dirty: bool,
    reframe: bool,
    hours: f64,
    drag: Option<Vec2>,
}

impl FormViewer {
    fn new() -> Result<Self, AppError> {
        let environment =
            EnvironmentSystem::new(EnvironmentOptions { time_of_day: 15.5 / 24.0, ..Default::default() })?;
        Ok(Self {
            library: FormAssetLibrary::new(),
            scene: RenderScene::new(),
            camera: OrbitCamera { yaw: 0.65, pitch: 0.38, ..Default::default() },
            environment,
            lighting: None,
            selected: 0,
            shading: ShadingModel::Lit,
            parameters: Vec::new(),
            instance: None,
            error: None,
            dirty: true,
            reframe: true,
            hours: 15.5,
            drag: None,
        })
    }

    /// Recompile the selected asset with the current parameters and replace the drawn instance.
    fn rebuild(&mut self) {
        self.dirty = false;
        let (path, text) = ASSETS[self.selected];
        let overrides: Map<String, Value> =
            self.parameters.iter().map(|parameter| (parameter.name.clone(), Value::from(parameter.value))).collect();
        let asset = match self.library.resolve(path, text, &overrides, self.shading) {
            Ok(asset) => asset,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        self.error = None;
        if self.parameters.is_empty() {
            self.parameters = scalar_parameters(path, text);
        }
        if self.reframe {
            self.reframe = false;
            let bounds = asset.bounds();
            self.camera.frame_positions(&[bounds.min, bounds.max]);
            self.camera.distance *= 1.2;
        }
        if let Some(previous) = self.instance.take() {
            self.scene.remove(previous);
        }
        self.instance = Some(self.scene.insert(MeshInstance::new(asset)));

        // A ground plane under the asset receives its shadow.
        let bounds = self.scene.get(self.instance.unwrap()).map(|instance| instance.asset.bounds());
        if let Some(bounds) = bounds {
            let size = (bounds.max - bounds.min).max_element() * 4.0;
            let ground = Transform::new(
                Vec3::new(
                    (bounds.min.x + bounds.max.x) * 0.5,
                    bounds.min.y - 0.05,
                    (bounds.min.z + bounds.max.z) * 0.5,
                ),
                Vec3::ZERO,
                Vec3::new(size, 0.1, size),
            );
            let keep: Vec<MeshInstanceId> =
                self.scene.iter().map(|(id, _)| id).filter(|id| Some(*id) != self.instance).collect();
            for id in keep {
                self.scene.remove(id);
            }
            self.scene.insert(
                MeshInstance::new(primitive_box()).with_transform(ground).with_tint(LinearColor::rgb(0.54, 0.6, 0.6)),
            );
        }
    }
}

impl App for FormViewer {
    fn input(&mut self, frame: &mut Frame<'_>) -> AppResult {
        let pointer = frame.input.pointer();
        if frame.input.action("orbit").value > 0.0 && pointer.active {
            if let Some(previous) = self.drag {
                let delta = pointer.position - previous;
                self.camera.yaw -= delta.x * 0.008;
                self.camera.pitch = (self.camera.pitch + delta.y * 0.006).clamp(-0.2, 1.45);
            }
            self.drag = Some(pointer.position);
        } else {
            self.drag = None;
        }
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        let environment = self.environment.step(f64::from(frame.time.elapsed_seconds))?;
        self.lighting = Some(environment_lighting(&environment, &SkyOptions::default()));
        if self.dirty {
            self.rebuild();
        }
        Ok(())
    }

    fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        let inputs = FrameInputs {
            environment: self.lighting.as_ref(),
            clear_color: LinearColor::new(0.09, 0.16, 0.2, 1.0),
            ..Default::default()
        };
        Ok(target.draw(&self.scene, &self.camera, &inputs)?)
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        egui::Window::new("FORM VIEWER").frame(joy_engine::theme::panel_frame()).default_width(280.0).show(
            context,
            |ui| {
                let mut selected = self.selected;
                egui::ComboBox::from_label("Asset").selected_text(asset_name(ASSETS[selected].0)).show_ui(ui, |ui| {
                    for (index, (path, _)) in ASSETS.iter().enumerate() {
                        ui.selectable_value(&mut selected, index, asset_name(path));
                    }
                });
                if selected != self.selected {
                    self.selected = selected;
                    self.parameters.clear();
                    self.dirty = true;
                    self.reframe = true;
                }
                ui.horizontal(|ui| {
                    for (label, shading) in
                        [("Lit", ShadingModel::Lit), ("Layered", ShadingModel::Layered), ("Unlit", ShadingModel::Unlit)]
                    {
                        if ui.selectable_label(self.shading == shading, label).clicked() && self.shading != shading {
                            self.shading = shading;
                            self.dirty = true;
                        }
                    }
                });
                if ui.add(egui::Slider::new(&mut self.hours, 0.0..=23.99).text("Solar time (h)")).changed() {
                    let _ = self.environment.set_time_of_day(self.hours / 24.0);
                }
                ui.add(egui::Slider::new(&mut self.camera.distance, 2.0..=200.0).logarithmic(true).text("Distance"));
                ui.separator();
                for parameter in &mut self.parameters {
                    let slider = egui::Slider::new(&mut parameter.value, parameter.minimum..=parameter.maximum)
                        .step_by(parameter.step)
                        .text(&parameter.name);
                    if ui.add(slider).changed() {
                        self.dirty = true;
                    }
                }
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(255, 120, 90), error);
                }
            },
        );
    }
}

/// File name without folders, for the asset menu.
fn asset_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Scalar parameters with bounds, read from an evaluation with default values.
fn scalar_parameters(path: &str, text: &str) -> Vec<Parameter> {
    let source = if path.ends_with(".formlab") {
        let document: Value = joy_engine::serde_json::from_str(text).unwrap_or(Value::Null);
        let overrides = document.get("overrides").cloned().unwrap_or(Value::Object(Map::new()));
        let source = document.get("source").and_then(Value::as_str).unwrap_or_default().to_owned();
        (source, overrides)
    } else {
        (text.to_owned(), Value::Object(Map::new()))
    };
    let Ok(geometry) = joy_engine::form::compile(&source.0, &source.1, None) else {
        return Vec::new();
    };
    geometry
        .parameters
        .iter()
        .filter(|parameter| parameter.get("type").and_then(Value::as_str) == Some("scalar"))
        .filter_map(|parameter| {
            let number = |name: &str| parameter.get(name).and_then(Value::as_f64);
            Some(Parameter {
                name: parameter.get("name")?.as_str()?.to_owned(),
                value: number("value")?,
                minimum: number("min")?,
                maximum: number("max")?,
                step: number("step").unwrap_or(0.0),
            })
        })
        .collect()
}

fn options() -> ShellOptions {
    let input = InputMap::new().action("orbit", [InputBinding::MouseButton(0)]);
    ShellOptions { title: "FORM VIEWER".into(), input, canvas_id: "form-viewer".into(), ..Default::default() }
}

fn start() {
    let result =
        FormViewer::new().map_err(|error| ShellError::App(error.to_string())).and_then(|app| run(options(), app));
    if let Err(error) = result {
        joy_engine::log::error!("{error}");
    }
}

joy_engine::web_main!(start);

fn main() {
    // Browsers start through `web_main!` instead.
    #[cfg(not(target_arch = "wasm32"))]
    start();
}
