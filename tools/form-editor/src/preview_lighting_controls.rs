// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Lighting inspector (UI module): direct light direction, linear
//! color, and intensity, studio environment intensity and rotation, shadow casting, and shadow quality. These settings
//! only redraw the preview; they never change a document, a save, or an export.

use joy_engine::development::components;
use joy_engine::development::{
    ColorControl,
    ComponentGroup,
    NumericDrafts,
    ScalarControl,
};
use joy_engine::egui::{
    self,
    RichText,
    Ui,
};
use joy_engine::rendering::ShadowQuality;
use joy_engine::runtime::theme::{
    colors,
    layout,
};

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
            "Studio reflections and light settings affect this preview only. Exports retain authored materials.",
        );
        ui.add_space(layout::SPACE_SM);
        let mut next = current;
        let [direction_x, direction_y, direction_z, red, green, blue, scalar_fields @ ..] = PREVIEW_LIGHTING_FIELDS;

        // Directions and colors use the same grouped components as vector and color parameters.
        let fields = [direction_x, direction_y, direction_z];
        let mut direction = fields.map(|field| current.get(field.key));
        setting_title(ui, "Light direction");
        ComponentGroup::xyz("preview-direction", &mut direction)
            .bounds(&fields.map(|field| field.minimum), &fields.map(|field| field.maximum))
            .step(direction_x.step)
            .name("Light direction")
            .show(ui, drafts);
        for (field, value) in fields.into_iter().zip(direction) {
            next = next.with(field.key, value);
        }
        ui.separator();

        let fields = [red, green, blue];
        let mut color = fields.map(|field| current.get(field.key));
        let minimums = fields.map(|field| field.minimum);
        let maximums = fields.map(|field| field.maximum);
        ui.horizontal(|ui| {
            setting_title(ui, "Light color");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ColorControl::new("preview-color-picker", &mut color, &minimums, &maximums)
                    .label("Light color")
                    .show(ui, drafts);
            });
        });
        ComponentGroup::rgba("preview-color", &mut color)
            .bounds(&minimums, &maximums)
            .step(red.step)
            .name("Light color")
            .show(ui, drafts);
        for (field, value) in fields.into_iter().zip(color) {
            next = next.with(field.key, value);
        }
        ui.separator();

        for field in scalar_fields {
            setting_title(ui, field.title);
            let mut value = current.get(field.key);
            ScalarControl::new(("preview-lighting", field.title), &mut value)
                .range(field.minimum, field.maximum)
                .step(field.step)
                .name(field.title)
                .show(ui, drafts);
            next = next.with(field.key, value);
            ui.add_space(layout::SPACE_XS);
            ui.separator();
        }
        components::checkbox(ui, &mut next.casts_shadow, "Cast shadows");
        components::field_title(ui, "Shadow quality");
        egui::ComboBox::from_id_salt("preview-quality")
            .selected_text(quality_name(next.quality))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for quality in [ShadowQuality::Low, ShadowQuality::Balanced, ShadowQuality::Cinematic] {
                    ui.selectable_value(&mut next.quality, quality, quality_name(quality));
                }
            })
            .response
            .on_hover_text("Shadow quality");
        ui.add_space(layout::SPACE_SM);
        if ui.add(components::button("Reset preview lighting")).clicked() {
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

/// Match parameter titles above their scalar, vector, or color controls.
fn setting_title(ui: &mut Ui, title: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(title).color(colors::TEXT).text_style(egui::TextStyle::Button).size(layout::TYPE_SM),
        )
        .wrap(),
    );
}
