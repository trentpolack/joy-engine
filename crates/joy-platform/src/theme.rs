// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Joy look for engine UI: navy panels with orange accents, matching the TypeScript engine's development widgets
//! and joy-apps' default theme.

/// Joy navy panel fill.
pub const NAVY: egui::Color32 = egui::Color32::from_rgb(0x14, 0x24, 0x2c);

/// Slightly lighter navy for panel borders and separators.
pub const NAVY_LINE: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x40, 0x4a);

/// Joy orange accent.
pub const ORANGE: egui::Color32 = egui::Color32::from_rgb(0xff, 0x6a, 0x00);

/// Primary panel text.
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe8, 0xee, 0xee);

/// Mint status accent.
pub const MINT: egui::Color32 = egui::Color32::from_rgb(0x8f, 0xe3, 0xc0);

/// Apply the Joy visuals to an egui context.
pub fn apply(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = NAVY;
    visuals.window_fill = NAVY;
    visuals.window_stroke = egui::Stroke::new(1.0, NAVY_LINE);
    visuals.selection.bg_fill = ORANGE;
    visuals.hyperlink_color = ORANGE;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ORANGE);
    visuals.widgets.active.bg_fill = ORANGE;
    context.set_visuals(visuals);
}

/// Standard translucent panel frame for HUD and overlay content.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(NAVY.gamma_multiply(0.92))
        .stroke(egui::Stroke::new(1.0, NAVY_LINE))
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(4))
}
