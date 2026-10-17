// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Variants inspector (UI module): up to 16 named takes of explicit parameter
//! overrides (meshes and materials included). Apply replaces current overrides; the script and the document material
//! palette stay shared by every take, and script defaults stay live for parameters a take leaves out.

use joy_engine::development::components;
use joy_engine::egui::{
    self,
    Ui,
};
use joy_engine::runtime::theme::tokens;

use crate::document::ParameterVariant;

/// What the author did in the Variants inspector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariantAction {
    /// Save current overrides under a new name.
    Save(String),
    /// Apply the take at an index.
    Apply(usize),
    /// Rename the take at an index.
    Rename(usize, String),
    /// Delete the take at an index.
    Delete(usize),
}

/// The typed name, the selected take, and the status line.
#[derive(Clone, Debug, Default)]
pub struct VariantControls {
    /// NAME THIS TAKE.
    pub name: String,
    /// The selected take.
    pub selected: usize,
    /// Status of the last action.
    pub status: String,
}

impl VariantControls {
    /// Clear the name and status (a new document).
    pub fn reset(&mut self) {
        self.name.clear();
        self.status.clear();
        self.selected = 0;
    }

    /// Draw the Variants tab and return the author's action.
    pub fn show(&mut self, ui: &mut Ui, variants: &[ParameterVariant]) -> Option<VariantAction> {
        let mut action = None;
        components::guidance(
            ui,
            "Keep different takes on the same script. Variants store parameter overrides, including mesh and material \
             inputs; script defaults stay live.",
        );
        ui.add_space(tokens::SPACE_SM);
        components::field_title(ui, "NAME THIS TAKE");
        ui.add(
            egui::TextEdit::singleline(&mut self.name)
                .hint_text("Weathered, tall, winter…")
                .char_limit(64)
                .desired_width(f32::INFINITY),
        );
        if ui.button("Save new variant +").clicked() {
            action = Some(VariantAction::Save(self.name.clone()));
        }
        ui.add_space(tokens::SPACE_SM);
        components::field_title(ui, "SAVED VARIANTS");
        self.selected = self.selected.min(variants.len().saturating_sub(1));
        let selected_text =
            variants.get(self.selected).map_or("No variants saved yet".to_owned(), |variant| variant.name.clone());
        ui.add_enabled_ui(!variants.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("variant-select")
                .selected_text(selected_text)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (index, variant) in variants.iter().enumerate() {
                        ui.selectable_value(&mut self.selected, index, variant.name.clone());
                    }
                });
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    action = Some(VariantAction::Apply(self.selected));
                }
                if ui.button("Rename").clicked() {
                    action = Some(VariantAction::Rename(self.selected, self.name.clone()));
                }
                if ui.button("Delete").clicked() {
                    action = Some(VariantAction::Delete(self.selected));
                }
            });
        });
        if !self.status.is_empty() {
            components::guidance(ui, self.status.clone());
        }
        ui.add_space(tokens::SPACE_SM);
        components::guidance(
            ui,
            "Apply replaces current overrides. Rename uses the name above. Your script and global material palette are \
             shared by every variant. Save to keep them together.",
        );
        action
    }
}
