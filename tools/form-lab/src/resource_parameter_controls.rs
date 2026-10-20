// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Mesh and material parameter groups (UI module). Mesh parameters
//! offer the current mesh, a unit box or sphere, and a geometry-only GLB import; material parameters edit linear RGBA
//! base color (alpha below one switches to BLEND), metallic, and roughness. Controls own no runtime state: every edit
//! publishes an explicit override.

use joy_engine::development::components;
use joy_engine::development::{
    ColorControl,
    ComponentGroup,
    ControlResponse,
    EditPhase,
    NumericDrafts,
    ScalarControl,
};
use joy_engine::egui::{
    self,
    RichText,
    Ui,
};
use joy_engine::runtime::theme::tokens;

use crate::parameter_controls::{
    ParameterAction,
    reset_override_button,
};
use crate::parameter_values::{
    MeshPreset,
    ParameterKind,
    ResourceParameter,
    material_factors,
    mesh_name,
    mesh_summary,
    with_material_color,
    with_material_factor,
};

/// One mesh or material group; `message` replaces the mesh status line while an import reports progress or failure.
pub fn resource_group(
    ui: &mut Ui,
    parameter: &mut ResourceParameter,
    title: &str,
    overridden: bool,
    message: Option<&str>,
    drafts: &mut NumericDrafts,
    actions: &mut Vec<ParameterAction>,
) {
    let mut color_response: Option<ControlResponse> = None;
    let (mut color, metallic, roughness) = material_factors(&parameter.value);
    ui.horizontal(|ui| {
        ui.label(RichText::new(parameter.kind.badge()).color(tokens::MUTED).size(tokens::TYPE_SM));
        ui.label(RichText::new(title).color(tokens::TEXT).strong());
        joy_engine::development::modified_marker(ui, overridden);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if reset_override_button(ui, overridden, &parameter.name).clicked() && overridden {
                actions.push(ParameterAction::Reset(parameter.name.clone()));
            }
            if parameter.kind == ParameterKind::Material {
                let response = ColorControl::new("picker", &mut color, &[0.0; 4], &[1.0; 4]).show(ui, drafts);
                if response.changed || response.phase != EditPhase::Idle {
                    color_response = Some(response);
                }
            }
        });
    });
    if parameter.kind == ParameterKind::Mesh {
        mesh_controls(ui, parameter, message, actions);
        return;
    }

    components::field_title(ui, "Base color · linear RGBA");
    let channels = ComponentGroup::rgba("base-color", &mut color)
        .bounds(&[0.0; 4], &[1.0; 4])
        .step(0.01)
        .without_sliders()
        .show(ui, drafts);
    let color_edit = color_response.unwrap_or(channels);
    if color_edit.changed || color_edit.committed() {
        publish(parameter, with_material_color(&parameter.value, color), color_edit.committed(), actions);
    }
    for (factor, label, mut value) in [("metallic", "Metallic", metallic), ("roughness", "Roughness", roughness)] {
        let response = ScalarControl::new(factor, &mut value).range(0.0, 1.0).step(0.01).label(label).show(ui, drafts);
        if response.changed || response.committed() {
            publish(parameter, with_material_factor(&parameter.value, factor, value), response.committed(), actions);
        }
    }
}

/// Apply a validated material edit to the displayed value and publish it as an override.
fn publish(
    parameter: &mut ResourceParameter,
    edited: Result<joy_engine::serde_json::Value, String>,
    commit: bool,
    actions: &mut Vec<ParameterAction>,
) {
    if let Ok(value) = edited {
        parameter.value = value.clone();
        actions.push(ParameterAction::Set { name: parameter.name.clone(), value, commit });
    }
}

/// The mesh source menu, GLB import, and status line.
fn mesh_controls(
    ui: &mut Ui,
    parameter: &mut ResourceParameter,
    message: Option<&str>,
    actions: &mut Vec<ParameterAction>,
) {
    ui.horizontal(|ui| {
        components::field_title(ui, "Mesh source");
        egui::ComboBox::from_id_salt("mesh-source").selected_text(mesh_name(&parameter.value).to_owned()).show_ui(
            ui,
            |ui| {
                let _ = ui.selectable_label(true, mesh_name(&parameter.value).to_owned());
                for preset in [MeshPreset::Box, MeshPreset::Sphere] {
                    if ui.selectable_label(false, preset.label()).clicked() {
                        let value = preset.value();
                        parameter.value = value.clone();
                        actions.push(ParameterAction::SetMesh { name: parameter.name.clone(), value });
                    }
                }
            },
        );
    });
    if ui.button("Import GLB geometry").on_hover_text(format!("{} GLB geometry", parameter.name)).clicked() {
        actions.push(ParameterAction::ImportMesh(parameter.name.clone()));
    }
    let status = message.map_or_else(|| mesh_summary(&parameter.value), str::to_owned);
    components::guidance(ui, status);
}
