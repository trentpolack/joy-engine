// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The shared Joy look for authoring tools (the TypeScript `components.ts` vocabulary: buttons, fields, panel headers,
//! toolbars, and status bars) as small egui helpers over [`theme::tokens`](crate::runtime::theme::tokens). Apply
//! [`theme::apply_authoring`](crate::runtime::theme::apply_authoring) once for the base widget style; these helpers add
//! the pieces egui has no style slot for.

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui};

use crate::runtime::hud_text::{self, Style};
use crate::runtime::theme::tokens;

/// A panel: the panel fill with a hairline border.
pub fn panel_frame() -> Frame {
    Frame::new()
        .fill(tokens::PANEL)
        .stroke(Stroke::new(tokens::BORDER_THIN, tokens::BORDER))
        .inner_margin(Margin::same(tokens::SPACE_MD as i8))
}

/// A toolbar or status bar strip: the canvas fill with small padding.
pub fn toolbar_frame() -> Frame {
    Frame::new()
        .fill(tokens::CANVAS)
        .stroke(Stroke::new(tokens::BORDER_THIN, tokens::BORDER))
        .inner_margin(Margin::symmetric(tokens::SPACE_MD as i8, tokens::SPACE_XS as i8))
}

/// A numbered panel heading in the lab style (`01 COMPOSE`): an accent index and a spaced, heavy uppercase title.
pub fn panel_heading(ui: &mut Ui, index: &str, title: &str) {
    let painter = ui.painter().clone();
    let heading = hud_text::layout(
        &painter,
        &[
            (index, Style::new(tokens::TYPE_SM, tokens::ACCENT).spacing(1.0).bold()),
            ("  ", Style::new(tokens::TYPE_SM, tokens::TEXT)),
            (&title.to_uppercase(), Style::new(tokens::TYPE_MD, tokens::TEXT).spacing(1.2).heavy()),
        ],
        f32::INFINITY,
        egui::Align::Min,
    );
    let (rect, _) = ui.allocate_exact_size(heading.size(), egui::Sense::hover());
    heading.paint(&painter, rect.min);
}

/// A primary action button (accent fill).
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(tokens::CREAM).strong())
        .fill(tokens::ACCENT)
        .stroke(Stroke::new(tokens::BORDER_THIN, tokens::ACCENT))
        .corner_radius(CornerRadius::same(tokens::RADIUS_SM))
}

/// A destructive action button (danger outline).
pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(tokens::DANGER))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(tokens::BORDER_THIN, tokens::DANGER))
}

/// A toggle button showing whether `selected` (tabs and panel toggles): selected buttons get the surface fill and an
/// accent underline.
pub fn toggle_button(ui: &mut Ui, selected: bool, text: impl Into<String>) -> egui::Response {
    let fill = if selected { tokens::SURFACE } else { tokens::BUTTON };
    let color = if selected { tokens::CREAM } else { tokens::MUTED };
    let response = ui.add(egui::Button::new(RichText::new(text.into()).color(color).size(tokens::TYPE_SM)).fill(fill));
    if selected {
        let rect = response.rect;
        ui.painter().line_segment(
            [rect.left_bottom() + egui::vec2(1.0, -1.0), rect.right_bottom() + egui::vec2(-1.0, -1.0)],
            Stroke::new(tokens::BORDER_FOCUS, tokens::ACCENT),
        );
    }
    response
}

/// A field title (small muted text above or beside a control).
pub fn field_title(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    ui.label(RichText::new(text.into()).color(tokens::MUTED).size(tokens::TYPE_SM))
}

/// Guidance text (extra-small muted text that wraps).
pub fn guidance(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text.into()).color(tokens::MUTED).size(tokens::TYPE_XS)).wrap())
}

/// An error message in the danger color that wraps.
pub fn error_text(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text.into()).color(tokens::DANGER).size(tokens::TYPE_SM)).wrap())
}

/// A small uppercase status badge (`LIVE`, `PENDING CHANGES`) in `color`.
pub fn status_badge(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    ui.label(RichText::new(text.to_uppercase()).color(color).size(tokens::TYPE_XS).strong())
}
