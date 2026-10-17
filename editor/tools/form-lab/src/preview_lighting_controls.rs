// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Lighting inspector (UI module): direct light direction, linear
//! color, and intensity, studio environment intensity and rotation, shadow casting, and shadow quality. These settings
//! only redraw the preview; they never change a document, a save, or an export.

use joy_engine::development::components;
use joy_engine::development::{
    NumericDrafts,
    ScalarControl,
};
use joy_engine::egui::{
    self,
    Ui,
};
use joy_engine::rendering::ShadowQuality;
use joy_engine::runtime::theme::tokens;

use crate::rendering::preview_lighting::{
    DEFAULT_PREVIEW_LIGHTING,
    PREVIEW_LIGHTING_FIELDS,
    PreviewLightingSettings,
    quality_name,
};

/// The validation message of the last rejected change.
#[derive(Clone, Debug, Default)]
pub struct PreviewLightingControls {
    /// Error from the last rejected setting.
    pub message: String,
}

impl PreviewLightingControls {
    /// Draw the Lighting tab; returns new settings when the author changed one (already validated).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        current: PreviewLightingSettings,
        drafts: &mut NumericDrafts,
    ) -> Option<PreviewLightingSettings> {
        components::field_title(ui, "PREVIEW LIGHTING");
        components::guidance(
            ui,
            "Studio reflections and light settings affect this preview only. Exports retain authored materials. \
             Shadows render on WebGPU and WebGL 2.",
        );
        ui.add_space(tokens::SPACE_SM);
        let mut next = current;
        for field in PREVIEW_LIGHTING_FIELDS {
            let mut value = current.get(field.key);
            ScalarControl::new(("preview-lighting", field.title), &mut value)
                .range(field.minimum, field.maximum)
                .step(field.step)
                .label(field.title)
                .show(ui, drafts);
            next = next.with(field.key, value);
        }
        ui.checkbox(&mut next.casts_shadow, "Cast shadows");
        egui::ComboBox::from_id_salt("preview-quality")
            .selected_text(quality_name(next.quality))
            .show_ui(ui, |ui| {
                for quality in [ShadowQuality::Low, ShadowQuality::Balanced, ShadowQuality::Cinematic] {
                    ui.selectable_value(&mut next.quality, quality, quality_name(quality));
                }
            })
            .response
            .on_hover_text("Shadow quality");
        if ui.button("Reset preview lighting").clicked() {
            next = DEFAULT_PREVIEW_LIGHTING;
        }
        if !self.message.is_empty() {
            components::error_text(ui, self.message.clone());
        }
        if next == current {
            return None;
        }
        match next.validate() {
            Ok(()) => {
                self.message.clear();
                Some(next)
            }
            Err(error) => {
                self.message = error;
                None
            }
        }
    }
}
