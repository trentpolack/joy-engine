// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The authoring kit on a real device through `HeadlessShell::step_with_ui`: a small tool lays out panes, a script
//! editor, and lab controls, receives synthetic pointer and keyboard input, and paints its UI over the capture.
//!
//! Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one the test prints a note and returns. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep the PNG capture. Native only.
#![cfg(not(target_arch = "wasm32"))]

use joy_engine::development::{
    ColorControl,
    ComponentGroup,
    NumberField,
    NumericDrafts,
    PanelLayout,
    PanelLayoutOptions,
    PropertyHistory,
    ScalarControl,
    ScriptDiagnostic,
    ScriptEditor,
    ScriptSyntax,
    components,
};
use joy_engine::egui;
use joy_engine::rendering::{
    FrameInputs,
    OrbitCamera,
    RenderScene,
};
use joy_engine::runtime::{
    App,
    AppResult,
    Frame,
    RenderTarget,
    StartContext,
    theme,
};
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};

const WIDTH: u32 = 1100;
const HEIGHT: u32 = 520;

/// Kit widgets in the three-pane lab layout.
struct KitTool {
    layout: PanelLayout,
    editor: ScriptEditor,
    script: String,
    power: f64,
    wind: [f64; 3],
    tint: [f64; 4],
    drafts: NumericDrafts,
    history: PropertyHistory<(f64, [f64; 3], [f64; 4])>,
    scene: RenderScene,
    field_center: egui::Pos2,
}

impl KitTool {
    fn new() -> Self {
        let mut editor = ScriptEditor::new("kit-script", ScriptSyntax::particle())
            .with_catalog(joy_engine::particles::particle_language_catalog());
        editor.set_diagnostics(vec![ScriptDiagnostic::error(2, 8, "Unknown name 'sizee'")]);
        Self {
            layout: PanelLayout::new(PanelLayoutOptions::default(), None),
            editor,
            script: "let angle = rand(0, tau);\nsize = sizee * power;\ncolor.a = 1 - t; // fade".to_owned(),
            power: 1.0,
            wind: [8.0, 0.0, 0.0],
            tint: [1.0, 0.65, 0.08, 1.0],
            drafts: NumericDrafts::default(),
            history: PropertyHistory::new((1.0, [8.0, 0.0, 0.0], [1.0, 0.65, 0.08, 1.0])),
            scene: RenderScene::new(),
            field_center: egui::Pos2::ZERO,
        }
    }

    fn snapshot(&self) -> (f64, [f64; 3], [f64; 4]) {
        (self.power, self.wind, self.tint)
    }
}

impl App for KitTool {
    fn start(&mut self, _context: &mut StartContext<'_>) -> AppResult {
        Ok(())
    }

    fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        let camera = OrbitCamera {
            distance: 6.0,
            ..Default::default()
        };
        Ok(target.draw(&self.scene, &camera, &FrameInputs::default())?)
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        theme::apply_editor(context);
        egui::Area::new(egui::Id::new("kit-root")).fixed_pos(egui::Pos2::ZERO).show(context, |ui| {
            let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH as f32, HEIGHT as f32));
            let panes = self.layout.panes(area);
            if let Some(rect) = panes.left {
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    components::panel_frame().show(ui, |ui| {
                        ui.set_min_size(rect.size() - egui::vec2(20.0, 20.0));
                        components::panel_heading(ui, "01", "Compose");
                        let response = self.editor.show(ui, &mut self.script);
                        components::guidance(ui, response.status_text());
                    });
                });
            }
            if let Some(rect) = panes.right {
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    components::panel_frame().show(ui, |ui| {
                        ui.set_min_size(rect.size() - egui::vec2(20.0, 20.0));
                        components::panel_heading(ui, "03", "Effect Parameters");
                        let mut committed = false;
                        components::field_title(ui, "power");
                        let response = ScalarControl::new("power", &mut self.power)
                            .range(0.25, 2.0)
                            .default_value(Some(1.0))
                            .step(0.1)
                            .show(ui, &mut self.drafts);
                        committed |= response.committed();
                        components::field_title(ui, "wind");
                        committed |= ComponentGroup::xyz("wind", &mut self.wind)
                            .bounds(&[-50.0; 3], &[50.0; 3])
                            .show(ui, &mut self.drafts)
                            .committed();
                        ui.horizontal(|ui| {
                            components::field_title(ui, "tint");
                            committed |= ColorControl::new("tint", &mut self.tint, &[0.0; 4], &[1.0; 4])
                                .show(ui, &self.drafts)
                                .committed();
                        });
                        committed |= ComponentGroup::rgba("tint-values", &mut self.tint)
                            .bounds(&[0.0; 4], &[1.0; 4])
                            .show(ui, &mut self.drafts)
                            .committed();
                        components::field_title(ui, "exact");
                        let field = NumberField::new("exact", &mut self.power)
                            .range(0.25, 2.0)
                            .step(0.1)
                            .show(ui, &mut self.drafts);
                        self.field_center = field.response.rect.center();
                        committed |= field.committed();
                        if committed {
                            let snapshot = self.snapshot();
                            self.history.commit(snapshot);
                        }
                    });
                });
            }
            self.layout.dividers(ui, &panes);
        });
    }
}

fn button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        events,
        ..Default::default()
    }
}

#[test]
fn kit_widgets_paint_and_respond_to_synthetic_input() {
    let options = HeadlessOptions {
        width: WIDTH,
        height: HEIGHT,
        ..Default::default()
    };
    let mut shell = match HeadlessShell::new(options) {
        Ok(shell) => shell,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return;
        }
    };
    let mut tool = KitTool::new();
    shell.start(&mut tool).expect("tool starts");
    shell.step_with_ui(&mut tool, input(vec![])).expect("first UI frame");
    shell.step_with_ui(&mut tool, input(vec![])).expect("layout settles");

    // Type an exact value into the field: click, replace the selection, confirm with Enter.
    let field = tool.field_center;
    shell
        .step_with_ui(
            &mut tool,
            input(vec![egui::Event::PointerMoved(field), button(field, true), button(field, false)]),
        )
        .unwrap();
    shell.step_with_ui(&mut tool, input(vec![egui::Event::Text("1.75".to_owned())])).unwrap();
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    };
    shell.step_with_ui(&mut tool, input(vec![enter])).unwrap();
    assert_eq!(tool.power, 1.75);
    assert!(tool.history.can_undo(), "the exact entry is one property edit");
    let restored = tool.history.undo(&tool.snapshot()).unwrap();
    assert_eq!(restored.0, 1.0);
    tool.power = restored.0;
    for _ in 0..12 {
        shell.step_with_ui(&mut tool, input(vec![])).unwrap();
    }

    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        shell.save_png(&std::path::Path::new(&directory).join("development-kit.png")).expect("png written");
    }
    let pixels = shell.capture().expect("readback");
    let pixel = |x: u32, y: u32| {
        let offset = ((y * WIDTH + x) * 4) as usize;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    };
    // The right panel paints the panel fill over the scene.
    let panel = pixel(WIDTH - 20, HEIGHT - 20);
    let expected = theme::colors::PANEL;
    assert!(
        panel
            .iter()
            .zip([expected.r(), expected.g(), expected.b()])
            .all(|(actual, wanted)| actual.abs_diff(wanted) <= 3),
        "panel fill {panel:?}"
    );
}
