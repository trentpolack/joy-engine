// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The shared Joy look for authoring tools: application chrome (title, header rule, project marker, status bar), list
//! rows, buttons, fields, panel headings, and toolbars as small egui helpers over
//! [`theme::tokens`](joy_ui::theme::tokens) and the semantic [`text_styles`](joy_ui::theme::text_styles). Apply
//! [`theme::apply_authoring`](joy_ui::theme::apply_authoring) once for the base widget style and fonts; these helpers
//! add the pieces egui has no style slot for, so Joy Editor, FORM LAB, and PARTICLE LAB share one vocabulary instead of
//! painting their own.

use egui::text::{
    LayoutJob,
    TextFormat,
};
use egui::{
    Color32,
    CornerRadius,
    FontId,
    Frame,
    Margin,
    Rect,
    RichText,
    Sense,
    Stroke,
    TextStyle,
    Ui,
    pos2,
    vec2,
};

use joy_ui::theme::{
    text_styles,
    tokens,
};

/// Letter spacing of application titles, in points.
const TITLE_SPACING: f32 = 0.6;

/// Letter spacing of uppercase panel titles and captions, in points.
const CAPTION_SPACING: f32 = 1.0;

/// Side of the project marker square.
const PROJECT_MARKER_SIZE: f32 = 8.0;

/// Width of a list row's leading accent bar when selected.
const ROW_ACCENT_WIDTH: f32 = 3.0;

// ---------------------------------------------------------------------------------------------------------------------
// Application chrome
// ---------------------------------------------------------------------------------------------------------------------

/// An application title in the header: the product name in display type and the accent color, then its version in an
/// outlined badge. Returns the area both occupy.
pub fn app_title(ui: &mut Ui, product: &str, version: &str) -> egui::Response {
    let display = style_font(ui, &text_styles::display(), tokens::TYPE_DISPLAY);
    let title = spaced(&product.to_uppercase(), display, tokens::ACCENT, TITLE_SPACING);
    let title = ui.label(title);
    let badge = Frame::new()
        .stroke(Stroke::new(tokens::BORDER_THIN, tokens::ACCENT))
        .inner_margin(Margin::symmetric(tokens::SPACE_XS as i8, 1))
        .show(ui, |ui| ui.label(RichText::new(version).color(tokens::ACCENT).text_style(text_styles::status())))
        .response;
    title.union(badge)
}

/// The accent rule under an application header, spanning `rect`'s full width along its bottom edge.
pub fn header_rule(ui: &Ui, rect: Rect) {
    let rule = Rect::from_min_max(pos2(rect.left(), rect.bottom() - tokens::BORDER_ACCENT), rect.right_bottom());
    ui.painter().rect_filled(rule, 0.0, tokens::ACCENT);
}

/// The small accent square that leads a project bar.
pub fn project_marker(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(PROJECT_MARKER_SIZE, PROJECT_MARKER_SIZE), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, tokens::ACCENT);
}

/// A status bar: `left` and `right` status text on the header fill with a hairline top edge, filling the `Ui`'s width.
pub fn status_bar(ui: &mut Ui, left: &str, right: &str) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, tokens::HEADER);
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(tokens::BORDER_THIN, tokens::LINE));
    ui.horizontal_centered(|ui| {
        ui.add_space(tokens::SPACE_MD);
        ui.label(spaced(&left.to_uppercase(), status_font(ui), tokens::ACCENT, CAPTION_SPACING));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(tokens::SPACE_MD);
            ui.label(spaced(&right.to_uppercase(), status_font(ui), tokens::MUTED, CAPTION_SPACING));
        });
    });
}

/// A selectable list row (Content Browser assets, outliner entities, emitter stacks): a full-width row with a
/// leading accent bar and the selected-row fill when `selected`, an optional muted `badge` on the right, and the
/// accent border under the pointer. Returns the row's click response.
pub fn list_row(ui: &mut Ui, selected: bool, label: &str, badge: Option<&str>) -> egui::Response {
    let height = tokens::CONTROL_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 0.0, tokens::SELECTED_ROW);
        let bar = Rect::from_min_size(rect.min, vec2(ROW_ACCENT_WIDTH, rect.height()));
        painter.rect_filled(bar, 0.0, tokens::ACCENT);
    }
    if response.hovered() {
        painter.rect_stroke(rect, 0.0, Stroke::new(tokens::BORDER_THIN, tokens::ACCENT), egui::StrokeKind::Inside);
    }
    let body = style_font(ui, &TextStyle::Body, tokens::TYPE_MD);
    let text_color = if selected || response.hovered() { tokens::CREAM } else { tokens::TEXT };
    painter.text(
        pos2(rect.left() + tokens::SPACE_MD + ROW_ACCENT_WIDTH, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        body,
        text_color,
    );
    if let Some(badge) = badge {
        painter.text(
            pos2(rect.right() - tokens::SPACE_MD, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            badge.to_uppercase(),
            status_font(ui),
            tokens::MUTED,
        );
    }
    response
}

// ---------------------------------------------------------------------------------------------------------------------
// Panels, buttons, and text
// ---------------------------------------------------------------------------------------------------------------------

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

/// A numbered panel heading in the lab style (`01 COMPOSE`): an accent index and a spaced uppercase title.
pub fn panel_heading(ui: &mut Ui, index: &str, title: &str) {
    let caption = style_font(ui, &text_styles::caption(), tokens::TYPE_SM);
    let title_font = style_font(ui, &text_styles::title(), tokens::TYPE_LG);
    let mut job = LayoutJob::default();
    append_spaced(&mut job, index, caption, tokens::ACCENT, CAPTION_SPACING);
    append_spaced(&mut job, "  ", title_font.clone(), tokens::TEXT, 0.0);
    append_spaced(&mut job, &title.to_uppercase(), title_font, tokens::TEXT, CAPTION_SPACING);
    ui.label(job);
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

/// A spaced uppercase caption in the caption style (section labels such as `EMITTER STACK`), in `color`.
pub fn caption(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    let caption = style_font(ui, &text_styles::caption(), tokens::TYPE_SM);
    ui.label(spaced(&text.to_uppercase(), caption, color, CAPTION_SPACING))
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
    ui.label(spaced(&text.to_uppercase(), status_font(ui), color, CAPTION_SPACING))
}

// ---------------------------------------------------------------------------------------------------------------------
// Text layout
// ---------------------------------------------------------------------------------------------------------------------

/// One run of text in `font` and `color` with `spacing` points between letters.
pub fn spaced(text: &str, font: FontId, color: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    append_spaced(&mut job, text, font, color, spacing);
    job
}

/// Append a letter-spaced run to `job`.
fn append_spaced(job: &mut LayoutJob, text: &str, font: FontId, color: Color32, spacing: f32) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
}

/// The status text font: the status style, which [`apply_authoring`](joy_ui::theme::apply_authoring) registers.
fn status_font(ui: &Ui) -> FontId {
    style_font(ui, &text_styles::status(), tokens::TYPE_XS)
}

/// The font of `style` in the `Ui`'s style, or the default proportional font at `fallback_size` when the authoring
/// look has not registered it (contexts that never called `apply_authoring`, such as some tests), instead of panicking.
fn style_font(ui: &Ui, style: &TextStyle, fallback_size: f32) -> FontId {
    ui.style().text_styles.get(style).cloned().unwrap_or_else(|| FontId::proportional(fallback_size))
}
