// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Parameters inspector (UI module), ported from `parameter-controls.ts`: one group per script parameter with
//! its type badge, default/modified indicator (hover for the script default), and per-parameter reset; bounded
//! scalars pair a slider with an exact field, XYZ components share one group, and colors pair linear RGBA channels
//! with a swatch and sRGB picker. Controls edit the build's resolved values in place and report overrides; the session
//! owns the document and property history.

use std::collections::{HashMap, HashSet};

use joy_engine::development::components;
use joy_engine::development::{ColorControl, ComponentGroup, ControlResponse, NumericDrafts, ScalarControl};
use joy_engine::egui::{self, RichText, Ui};
use joy_engine::runtime::theme::tokens;
use joy_engine::serde_json::Value as Json;

use crate::parameter_values::{NumericParameter, Parameter, ParameterKind, parameter_title};
use crate::resource_parameter_controls::resource_group;

/// What the author did in the Parameters inspector this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum ParameterAction {
    /// A property override changed; `commit` ends the gesture (record history).
    Set {
        /// Parameter name.
        name: String,
        /// The new override.
        value: Json,
        /// Whether to record property history now.
        commit: bool,
    },
    /// A mesh override changed (meshes stay outside property history).
    SetMesh {
        /// Parameter name.
        name: String,
        /// The mesh resource.
        value: Json,
    },
    /// Remove one override (the script default returns on the next build).
    Reset(String),
    /// Open a GLB for a mesh parameter.
    ImportMesh(String),
    /// Undo the last property edit.
    Undo,
    /// Redo the last undone property edit.
    Redo,
}

/// Inspector state that survives builds: the search text and resource status lines.
#[derive(Clone, Debug, Default)]
pub struct ParameterControls {
    /// Parameter search text.
    pub search: String,
    /// Per-parameter status lines (GLB import progress and errors).
    pub messages: HashMap<String, String>,
    signature: String,
    rects: HashMap<String, egui::Rect>,
}

impl ParameterControls {
    /// Forget search, messages, and drafts (a new document).
    pub fn clear(&mut self, drafts: &mut NumericDrafts) {
        self.search.clear();
        self.messages.clear();
        self.signature.clear();
        drafts.clear();
    }

    /// Where a parameter's main control (or `undo`, `redo`, `search`) was drawn last frame.
    pub fn control_rect(&self, name: &str) -> Option<egui::Rect> {
        self.rects.get(name).copied()
    }

    /// Draw the Parameters tab and return the author's actions.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        parameters: &mut [Parameter],
        signature: String,
        overridden: &HashSet<String>,
        drafts: &mut NumericDrafts,
        can_undo: bool,
        can_redo: bool,
        guidance: &str,
    ) -> Vec<ParameterAction> {
        let mut actions = Vec::new();
        if signature != self.signature {
            // Controls rebuild when declarations change, discarding drafts of fields that no longer exist.
            self.signature = signature;
            drafts.clear();
        }
        self.rects.clear();
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.search).hint_text("Find a parameter…").desired_width(f32::INFINITY),
        );
        self.rects.insert("search".into(), search.rect);
        ui.add_space(tokens::SPACE_SM);
        components::guidance(ui, guidance);
        ui.add_space(tokens::SPACE_SM);
        ui.horizontal(|ui| {
            let undo = ui.add_enabled(can_undo, egui::Button::new("Undo property"));
            self.rects.insert("undo".into(), undo.rect);
            if undo.clicked() {
                actions.push(ParameterAction::Undo);
            }
            let redo = ui.add_enabled(can_redo, egui::Button::new("Redo property"));
            self.rects.insert("redo".into(), redo.rect);
            if redo.clicked() {
                actions.push(ParameterAction::Redo);
            }
        });
        egui::CollapsingHeader::new(
            RichText::new("Tuning keys & defaults").color(tokens::ACCENT).size(tokens::TYPE_SM),
        )
        .id_salt("form-lab-tuning-help")
        .show(ui, |ui| {
            components::guidance(
                ui,
                "Type a value · Shift + arrows fine · Alt + arrows coarse. Reset returns to the script default; \
                     source has separate editor undo. Color channels are linear RGBA; A controls opacity.",
            );
        });
        ui.add_space(tokens::SPACE_SM);

        if parameters.is_empty() {
            components::guidance(ui, "Add a param declaration to give your script a control.");
            return actions;
        }
        let query = self.search.trim().to_lowercase().replace('_', " ");
        let mut matches = 0;
        for parameter in parameters.iter_mut() {
            let title = parameter_title(parameter.name());
            if !query.is_empty() && !title.to_lowercase().contains(&query) {
                continue;
            }
            matches += 1;
            ui.separator();
            let overridden = overridden.contains(parameter.name());
            let name = parameter.name().to_owned();
            let group = ui.push_id(("form-lab-parameter", name.clone()), |ui| match parameter {
                Parameter::Numeric(numeric) => {
                    Some(numeric_group(ui, numeric, &title, overridden, drafts, &mut actions))
                }
                Parameter::Resource(resource) => {
                    let message = self.messages.get(&resource.name).cloned();
                    resource_group(ui, resource, &title, overridden, message.as_deref(), drafts, &mut actions);
                    None
                }
            });
            self.rects.insert(name, group.inner.unwrap_or(group.response.rect));
        }
        if matches == 0 {
            ui.label(RichText::new("No matching parameters.").color(tokens::MUTED));
        }
        actions
    }
}

/// One scalar, vector, or color group.
fn numeric_group(
    ui: &mut Ui,
    parameter: &mut NumericParameter,
    title: &str,
    overridden: bool,
    drafts: &mut NumericDrafts,
    actions: &mut Vec<ParameterAction>,
) -> egui::Rect {
    let modified = parameter.modified();
    let mut response: Option<ControlResponse> = None;
    let minimums = vec![parameter.minimum; parameter.value.len()];
    let maximums = vec![parameter.maximum; parameter.value.len()];
    ui.horizontal(|ui| {
        ui.label(RichText::new(parameter.kind.badge()).color(tokens::MUTED).size(tokens::TYPE_SM))
            .on_hover_text(format!("{} parameter", parameter.kind.name()));
        ui.label(RichText::new(title).color(tokens::TEXT).strong());
        joy_engine::development::modified_marker(ui, modified);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let defaults: Vec<String> =
                parameter.default_value.iter().map(|value| joy_engine::core::format_js_number(*value)).collect();
            let reset = reset_override_button(ui, overridden, &parameter.name);
            if reset.clicked() && overridden {
                actions.push(ParameterAction::Reset(parameter.name.clone()));
            }
            let state = if modified {
                "Modified"
            } else if overridden {
                "Override"
            } else {
                "Default"
            };
            ui.label(
                RichText::new(state).color(if modified { tokens::ACCENT } else { tokens::MUTED }).size(tokens::TYPE_SM),
            )
            .on_hover_text(format!("Script default: {}", defaults.join(", ")));
            if parameter.kind == ParameterKind::Color {
                let picked = ColorControl::new("picker", &mut parameter.value, &minimums, &maximums).show(ui, drafts);
                if picked.changed || picked.phase != joy_engine::development::EditPhase::Idle {
                    response = Some(picked);
                }
            }
        });
    });
    let edited = match parameter.kind {
        ParameterKind::Scalar => {
            let mut value = parameter.value.first().copied().unwrap_or_default();
            let edited = ScalarControl::new("value", &mut value)
                .range(parameter.minimum, parameter.maximum)
                .step(step_or_default(parameter.step))
                .show(ui, drafts);
            if let Some(slot) = parameter.value.first_mut() {
                *slot = value;
            }
            edited
        }
        ParameterKind::Vector => ComponentGroup::xyz("components", &mut parameter.value)
            .bounds(&minimums, &maximums)
            .step(step_or_default(parameter.step))
            .show(ui, drafts),
        _ => ComponentGroup::rgba("components", &mut parameter.value)
            .bounds(&minimums, &maximums)
            .step(step_or_default(parameter.step))
            .show(ui, drafts),
    };
    let control_rect = edited.response.rect;
    let response = match response {
        Some(picked) => picked,
        None => edited,
    };
    if response.changed || response.committed() {
        actions.push(ParameterAction::Set {
            name: parameter.name.clone(),
            value: parameter.override_value(&parameter.value),
            commit: response.committed(),
        });
    }
    control_rect
}

/// A usable control step: FORM's declared step, or a hundredth when a script declares none.
fn step_or_default(step: f64) -> f64 {
    if step.is_finite() && step > 0.0 { step } else { 0.01 }
}

/// The per-parameter reset: a circular arrow, enabled while the parameter has an override, that removes it.
pub fn reset_override_button(ui: &mut Ui, overridden: bool, name: &str) -> egui::Response {
    let size = egui::Vec2::splat(20.0);
    let sense = if overridden { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let color = if !overridden {
        tokens::BORDER
    } else if response.hovered() {
        tokens::ACCENT_HOVER
    } else {
        tokens::MUTED
    };
    let painter = ui.painter();
    let center = rect.center();
    let radius = 6.0;
    let stroke = egui::Stroke::new(1.5, color);
    let points: Vec<egui::Pos2> = (0..=20)
        .map(|step| {
            let angle = -0.6 + (step as f32 / 20.0) * 4.9;
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
    let tip = center + egui::vec2((-0.6_f32).cos(), (-0.6_f32).sin()) * radius;
    painter.line_segment([tip, tip + egui::vec2(-4.0, -1.0)], stroke);
    painter.line_segment([tip, tip + egui::vec2(0.5, -4.0)], stroke);
    response.on_hover_text(format!("Reset {name} to script default"))
}
