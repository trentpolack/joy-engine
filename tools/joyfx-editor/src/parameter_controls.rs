// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Effect parameter cards, built from the kit's lab controls: a type menu, the
//! name, a linear-RGBA swatch for colors, collapse and delete buttons, then the values (a bounded scalar, or XYZ and
//! RGBA component groups) and the per-component bounds beneath. Collapsing hides the editors without changing the
//! effect and keeps a compact summary.

use std::collections::HashMap;

use joy_engine::development::lab_values::format_number;
use joy_engine::development::{
    ColorControl,
    ComponentGroup,
    NumberField,
    ScalarControl,
};
use joy_engine::egui::{
    self,
    FontId,
    RichText,
    Sense,
    Stroke,
    Ui,
    pos2,
    vec2,
};
use joy_engine::particles::ParticleParameter;
use joy_engine::runtime::theme::{
    colors,
    layout,
};

use crate::document::{
    self,
    Bound,
    ParameterKind,
    bound_limits,
    check_bound,
    clamp_parameter_value,
    parameter_components,
};
use crate::session::JoyfxEditor;

/// A structural change a card requested; applied after the list is drawn.
enum CardAction {
    Collapse(String, bool),
    Delete(String),
    Rename(String, String),
    Kind(String, ParameterKind),
}

/// Card interface state that is not document data.
#[derive(Default)]
pub struct ParameterPanel {
    /// Name drafts being typed, by parameter.
    name_drafts: HashMap<String, String>,
}

impl ParameterPanel {
    /// Draw every parameter card and apply the author's edits to `lab`.
    pub fn show(&mut self, ui: &mut Ui, lab: &mut JoyfxEditor) {
        let names: Vec<String> = lab.asset().parameters.keys().cloned().collect();
        if names.is_empty() {
            ui.label(RichText::new("No named parameters.").color(colors::MUTED));
            return;
        }
        let mut actions = Vec::new();
        let mut changed = false;
        let mut committed = false;
        for name in &names {
            let collapsed = lab.collapsed_parameters.contains(name);
            let (asset, drafts) = lab.edit_parts();
            let Some(parameter) = asset.parameters.get_mut(name) else {
                continue;
            };
            ui.push_id(("parameter", name), |ui| {
                let kind = ParameterKind::of(parameter);
                // Heading: type, name, swatch, collapse, delete.
                ui.horizontal(|ui| {
                    let mut selected = kind;
                    egui::ComboBox::from_id_salt("type")
                        .width(40.0)
                        .selected_text(RichText::new(kind.code()).color(colors::MUTED).size(layout::TYPE_XS))
                        .show_ui(ui, |ui| {
                            for option in [ParameterKind::Scalar, ParameterKind::Vector, ParameterKind::Color] {
                                ui.selectable_value(&mut selected, option, option.label());
                            }
                        })
                        .response
                        .on_hover_text(format!("{name} type"));
                    if selected != kind {
                        actions.push(CardAction::Kind(name.clone(), selected));
                    }
                    let swatch_width = if kind == ParameterKind::Color { 36.0 } else { 0.0 };
                    let draft = self.name_drafts.entry(name.clone()).or_insert_with(|| name.clone());
                    let field_width = (ui.available_width() - swatch_width - 64.0).max(60.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(draft)
                            .desired_width(field_width)
                            .font(FontId::proportional(layout::TYPE_MD)),
                    );
                    if response.lost_focus() {
                        if draft.trim() != name {
                            actions.push(CardAction::Rename(name.clone(), draft.clone()));
                        }
                        *draft = name.clone();
                    }
                    if let ParticleParameter::Color {
                        value,
                        min,
                        max,
                    } = parameter
                    {
                        let response =
                            ColorControl::new("swatch", value, min, max).label(name.as_str()).show(ui, drafts);
                        changed |= response.changed;
                        committed |= response.committed();
                    }
                    let label = if collapsed { "Expand" } else { "Collapse" };
                    if triangle_button(ui, collapsed).on_hover_text(format!("{label} {name} values")).clicked() {
                        actions.push(CardAction::Collapse(name.clone(), !collapsed));
                    }
                    let delete = ui.add(egui::Button::new(RichText::new("×").color(colors::DANGER)).frame(false));
                    if delete.on_hover_text(format!("Delete {name}")).clicked() {
                        actions.push(CardAction::Delete(name.clone()));
                    }
                });

                if collapsed {
                    let summary = parameter_summary(parameter);
                    if !summary.is_empty() {
                        ui.label(RichText::new(summary).monospace().color(colors::MUTED).size(layout::TYPE_SM));
                    }
                } else {
                    // Values.
                    let response = match parameter {
                        ParticleParameter::Scalar {
                            value,
                            min,
                            max,
                        } => ScalarControl::new("value", value).range(*min, *max).name(name.as_str()).show(ui, drafts),
                        ParticleParameter::Vector3 {
                            value,
                            min,
                            max,
                        } => ComponentGroup::xyz("value", value).bounds(min, max).name(name.as_str()).show(ui, drafts),
                        ParticleParameter::Color {
                            value,
                            min,
                            max,
                        } => ComponentGroup::rgba("value", value).bounds(min, max).name(name.as_str()).show(ui, drafts),
                    };
                    changed |= response.changed;
                    committed |= response.committed();

                    // Bounds.
                    let title = if kind == ParameterKind::Color {
                        "Bounds · Linear RGB (HDR) · Alpha 0–1"
                    } else {
                        "Bounds"
                    };
                    egui::CollapsingHeader::new(RichText::new(title).size(layout::TYPE_XS).color(colors::MUTED))
                        .id_salt("bounds")
                        .show(ui, |ui| {
                            let labels: &[&str] = match kind {
                                ParameterKind::Scalar => &[""],
                                ParameterKind::Vector => &["X", "Y", "Z"],
                                ParameterKind::Color => &["R", "G", "B", "A"],
                            };
                            for (component, label) in labels.iter().enumerate() {
                                let (lower, upper) = bound_limits(parameter, component);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(*label).monospace().color(colors::MUTED).size(layout::TYPE_XS),
                                    );
                                    for bound in [Bound::Minimum, Bound::Maximum] {
                                        let snapshot = parameter.clone();
                                        let (_, minimums, maximums) = parameter_components(parameter);
                                        let slot = if bound == Bound::Minimum {
                                            &mut minimums[component]
                                        } else {
                                            &mut maximums[component]
                                        };
                                        ui.label(
                                            RichText::new(if bound == Bound::Minimum { "MIN" } else { "MAX" })
                                                .color(colors::MUTED)
                                                .size(layout::TYPE_XS),
                                        );
                                        let response = NumberField::new((bound == Bound::Minimum, component), slot)
                                            .range(lower, upper)
                                            .step(0.1)
                                            .width(68.0)
                                            .label(
                                                format!(
                                                    "{name} {label} {}",
                                                    if bound == Bound::Minimum { "minimum" } else { "maximum" }
                                                )
                                                .trim()
                                                .to_owned(),
                                            )
                                            .check(move |value| check_bound(&snapshot, bound, component, value))
                                            .show(ui, drafts);
                                        if response.changed {
                                            // Tightened bounds clamp the value immediately.
                                            clamp_parameter_value(parameter);
                                        }
                                        changed |= response.changed;
                                        committed |= response.committed();
                                    }
                                });
                            }
                        });
                }
                ui.add_space(2.0);
                let rect = ui.available_rect_before_wrap();
                ui.painter().line_segment(
                    [pos2(rect.left(), rect.top()), pos2(rect.right(), rect.top())],
                    Stroke::new(1.0, colors::BORDER),
                );
                ui.add_space(layout::SPACE_MD);
            });
        }

        lab.property_edit(changed, committed);
        for action in actions {
            let result = match action {
                CardAction::Collapse(name, collapse) => {
                    if collapse {
                        lab.collapsed_parameters.insert(name);
                    } else {
                        lab.collapsed_parameters.remove(&name);
                    }
                    Ok(false)
                }
                CardAction::Delete(name) => document::delete_parameter(lab.asset_mut(), &name).map(|()| {
                    lab.collapsed_parameters.remove(&name);
                    self.name_drafts.remove(&name);
                    true
                }),
                CardAction::Rename(name, next) => {
                    document::rename_parameter(lab.asset_mut(), &name, &next).map(|renamed| {
                        if let Some(renamed) = &renamed
                            && lab.collapsed_parameters.remove(&name)
                        {
                            lab.collapsed_parameters.insert(renamed.clone());
                        }
                        self.name_drafts.remove(&name);
                        renamed.is_some()
                    })
                }
                CardAction::Kind(name, kind) => {
                    document::change_parameter_kind(lab.asset_mut(), &name, kind).map(|()| true)
                }
            };
            match result {
                Ok(true) => lab.structure_changed(),
                Ok(false) => {}
                Err(message) => lab.show_error(message),
            }
        }
    }
}

/// The compact value shown while a card is collapsed; read-only rounding never changes the value. Colors show their
/// swatch instead.
pub fn parameter_summary(parameter: &ParticleParameter) -> String {
    let rounded = |value: f64| format_number((value * 1000.0).round() / 1000.0);
    match parameter {
        ParticleParameter::Scalar {
            value,
            ..
        } => rounded(*value),
        ParticleParameter::Vector3 {
            value,
            ..
        } => ["X", "Y", "Z"]
            .iter()
            .zip(value)
            .map(|(axis, value)| format!("{axis} {}", rounded(*value)))
            .collect::<Vec<_>>()
            .join(" · "),
        ParticleParameter::Color {
            ..
        } => String::new(),
    }
}

/// A small up (expanded) or down (collapsed) triangle button.
fn triangle_button(ui: &mut Ui, collapsed: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let color = if response.hovered() { colors::ACCENT_HOVER } else { colors::MUTED };
    let center = rect.center();
    let size = 4.5;
    let points = if collapsed {
        vec![center + vec2(-size, -size * 0.5), center + vec2(size, -size * 0.5), center + vec2(0.0, size * 0.7)]
    } else {
        vec![center + vec2(-size, size * 0.5), center + vec2(size, size * 0.5), center + vec2(0.0, -size * 0.7)]
    };
    ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    response
}
