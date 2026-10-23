// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The shared Joy look for authoring tools: application chrome (title, header rule, project marker, status bar), list
//! rows, buttons, fields, panel headings, and toolbars as small egui helpers over
//! [`colors`], [`layout`], and semantic [`text_styles`]. Apply
//! [`theme::apply_editor`](joy_ui::theme::apply_editor) once for the base widget style and fonts; these helpers
//! add the pieces egui has no style slot for, so Joy Editor, FORM EDITOR, and JOYFX EDITOR share one vocabulary instead of
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
    colors,
    layout,
    text_styles,
};

/// Letter spacing of application titles, in points.
const TITLE_SPACING: f32 = -1.0;

/// Compact tool title size, matching the standalone browser header.
const TITLE_SIZE: f32 = 27.0;

/// Letter spacing of uppercase panel titles and captions, in points.
const CAPTION_SPACING: f32 = 1.0;

/// Side of the project marker square.
const PROJECT_MARKER_SIZE: f32 = 8.0;

/// Width of a list row's leading accent bar when selected.
const ROW_ACCENT_WIDTH: f32 = 3.0;

/// Space between a shortcut button's label and its keycap.
const KEYCAP_GAP: f32 = 7.0;

/// Horizontal padding inside a keycap, and its narrowest width (a single key reads as a square).
const KEYCAP_PADDING: f32 = 4.0;
const KEYCAP_MINIMUM_WIDTH: f32 = 16.0;

// ---------------------------------------------------------------------------------------------------------------------
// Application chrome
// ---------------------------------------------------------------------------------------------------------------------

/// An application title in the header: the product name in display type and the accent color, then its version in an
/// outlined badge. Returns the area both occupy.
pub fn app_title(ui: &mut Ui, product: &str, version: &str) -> egui::Response {
    let display = style_font(ui, &text_styles::display(), layout::TYPE_DISPLAY);
    let title = spaced(&product.to_uppercase(), FontId::new(TITLE_SIZE, display.family), colors::ACCENT, TITLE_SPACING);
    let title = ui.label(title);
    ui.add_space(layout::SPACE_SM);
    let badge = ui.label(RichText::new(version).color(colors::ACCENT).text_style(text_styles::status()));
    ui.painter().rect_stroke(
        badge.rect.expand2(vec2(4.0, 3.0)),
        0.0,
        Stroke::new(layout::BORDER_THIN, colors::ACCENT),
        egui::StrokeKind::Outside,
    );
    title.union(badge)
}

/// The accent rule under an application header, spanning `rect`'s full width along its bottom edge.
pub fn header_rule(ui: &Ui, rect: Rect) {
    let rule = Rect::from_min_max(pos2(rect.left(), rect.bottom() - layout::BORDER_ACCENT), rect.right_bottom());
    ui.painter().rect_filled(rule, 0.0, colors::ACCENT);
}

/// The small accent square that leads a project bar.
pub fn project_marker(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(PROJECT_MARKER_SIZE, PROJECT_MARKER_SIZE), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, colors::ACCENT);
}

/// A status bar: `left` and `right` status text on the header fill with a hairline top edge, filling the `Ui`'s width.
pub fn status_bar(ui: &mut Ui, left: &str, right: &str) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, colors::HEADER);
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(layout::BORDER_THIN, colors::LINE));
    ui.horizontal_centered(|ui| {
        ui.add_space(layout::SPACE_MD);
        ui.label(spaced(&left.to_uppercase(), status_font(ui), colors::ACCENT, CAPTION_SPACING));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(layout::SPACE_MD);
            ui.label(spaced(&right.to_uppercase(), status_font(ui), colors::MUTED, CAPTION_SPACING));
        });
    });
}

/// A selectable list row (Content Browser assets, outliner entities, emitter stacks): a full-width row with a
/// leading accent bar and the selected-row fill when `selected`, an optional muted `badge` on the right, and the
/// accent border under the pointer. Returns the row's click response.
pub fn list_row(ui: &mut Ui, selected: bool, label: &str, badge: Option<&str>) -> egui::Response {
    let height = layout::CONTROL_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 0.0, colors::SELECTED_ROW);
        let bar = Rect::from_min_size(rect.min, vec2(ROW_ACCENT_WIDTH, rect.height()));
        painter.rect_filled(bar, 0.0, colors::ACCENT);
    }
    if response.hovered() {
        painter.rect_stroke(rect, 0.0, Stroke::new(layout::BORDER_THIN, colors::ACCENT), egui::StrokeKind::Inside);
    }
    let body = style_font(ui, &TextStyle::Body, layout::TYPE_MD);
    let text_color = if selected || response.hovered() { colors::CREAM } else { colors::TEXT };
    painter.text(
        pos2(rect.left() + layout::SPACE_MD + ROW_ACCENT_WIDTH, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        body,
        text_color,
    );
    if let Some(badge) = badge {
        painter.text(
            pos2(rect.right() - layout::SPACE_MD, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            badge.to_uppercase(),
            status_font(ui),
            colors::MUTED,
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
        .fill(colors::PANEL)
        .stroke(Stroke::new(layout::BORDER_THIN, colors::BORDER))
        .inner_margin(Margin::same(layout::SPACE_MD as i8))
}

/// A toolbar or status bar strip: the panel fill with small padding.
pub fn toolbar_frame() -> Frame {
    Frame::new()
        .fill(colors::PANEL)
        .stroke(Stroke::new(layout::BORDER_THIN, colors::BORDER))
        .inner_margin(Margin::symmetric(layout::SPACE_MD as i8, layout::SPACE_XS as i8))
}

/// A major panel heading in the tool editor style: a bold, spaced uppercase title.
pub fn panel_heading(ui: &mut Ui, title: &str) {
    let title_font = FontId::new(layout::TYPE_LG, text_styles::strong_family());
    let mut job = LayoutJob::default();
    append_spaced(&mut job, &title.to_uppercase(), title_font, colors::TEXT, CAPTION_SPACING);
    ui.label(job);
}

/// A standard workbench button with the explicit bold face used by the browser tool editors.
pub fn button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).text_style(TextStyle::Button))
        .min_size(vec2(0.0, layout::CONTROL_HEIGHT))
}

/// A workbench button that names its keyboard shortcut: the label in the button face, then `key` in a small outlined
/// monospace keycap, so the shortcut reads as a key rather than part of the name. Hover text should describe the action
/// without repeating the key; screen readers hear `label (key)`.
pub fn shortcut_button(ui: &mut Ui, label: &str, key: &str) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let label_galley = egui::WidgetText::from(RichText::new(label).text_style(TextStyle::Button)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    let key_galley = ui.painter().layout_no_wrap(key.to_owned(), FontId::monospace(layout::TYPE_XS), colors::MUTED);
    let keycap_size = vec2(
        (key_galley.size().x + (KEYCAP_PADDING * 2.0)).max(KEYCAP_MINIMUM_WIDTH),
        key_galley.size().y + KEYCAP_PADDING,
    );
    let width = (padding.x * 2.0) + label_galley.size().x + KEYCAP_GAP + keycap_size.x;
    let height = layout::CONTROL_HEIGHT.max(label_galley.size().y + (padding.y * 2.0));
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), format!("{label} ({key})"))
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }

    // The button face matches `button`: egui's interaction visuals for fill, outline, and text.
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
    let label_position = pos2(rect.left() + padding.x, rect.center().y - (label_galley.size().y * 0.5));
    let keycap = Rect::from_min_size(
        pos2(label_position.x + label_galley.size().x + KEYCAP_GAP, rect.center().y - (keycap_size.y * 0.5)),
        keycap_size,
    );
    painter.galley(label_position, label_galley, visuals.text_color());
    painter.rect_stroke(
        keycap,
        CornerRadius::same(layout::RADIUS_SM),
        Stroke::new(layout::BORDER_THIN, colors::BORDER),
        egui::StrokeKind::Inside,
    );
    painter.galley(keycap.center() - (key_galley.size() * 0.5), key_galley, colors::MUTED);
    response
}

/// A native checkbox with the tool editors' orange checked fill and compact label, retaining egui's input and accessibility.
pub fn checkbox(ui: &mut Ui, value: &mut bool, text: &str) -> egui::Response {
    // Draw in the caller's layout so wrapped toolbars can measure the whole checkbox before placing it.
    let previous = ui.style().clone();
    ui.spacing_mut().icon_width = 13.0;
    ui.spacing_mut().icon_width_inner = 9.0;
    let widgets = &mut ui.visuals_mut().widgets;
    for visuals in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active] {
        visuals.bg_fill = if *value { colors::ACCENT } else { colors::FIELD };
        visuals.fg_stroke.color = colors::FIELD;
        visuals.corner_radius = CornerRadius::same(layout::RADIUS_SM);
    }
    let response = ui.checkbox(value, RichText::new(text).color(colors::MUTED).size(layout::TYPE_SM));
    ui.set_style(previous);
    response
}

/// A primary action button (accent fill).
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(colors::ON_ACCENT).text_style(TextStyle::Button))
        .min_size(vec2(0.0, layout::CONTROL_HEIGHT))
        .fill(colors::ACCENT)
        .stroke(Stroke::new(layout::BORDER_THIN, colors::ACCENT))
        .corner_radius(CornerRadius::same(layout::RADIUS_SM))
}

/// A destructive action button (danger outline).
pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(colors::DANGER))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(layout::BORDER_THIN, colors::DANGER))
}

/// A preview mode toggle: selected actions have an orange outline and text; inactive actions stay quiet.
pub fn toggle_button(ui: &mut Ui, selected: bool, text: impl Into<String>) -> egui::Response {
    let fill = if selected { colors::SURFACE } else { Color32::TRANSPARENT };
    let color = if selected { colors::ACCENT } else { colors::MUTED };
    let response = ui.add(
        egui::Button::new(
            RichText::new(text.into()).color(color).size(layout::TYPE_XS).family(text_styles::strong_family()),
        )
        .min_size(vec2(0.0, layout::CONTROL_HEIGHT))
        .fill(fill)
        .stroke(Stroke::NONE),
    );
    if selected || response.hovered() || response.has_focus() {
        let color = if selected || response.has_focus() { colors::ACCENT } else { colors::BORDER };
        ui.painter().rect_stroke(response.rect, 0.0, Stroke::new(layout::BORDER_THIN, color), egui::StrokeKind::Inside);
    }
    response
}

/// A field title (small muted text above or beside a control).
pub fn field_title(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    ui.label(RichText::new(text.into()).color(colors::MUTED).size(layout::TYPE_SM))
}

/// A spaced uppercase caption in the caption style (section labels such as `EMITTER STACK`), in `color`.
pub fn caption(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    let caption = style_font(ui, &text_styles::caption(), layout::TYPE_SM);
    ui.label(spaced(&text.to_uppercase(), caption, color, CAPTION_SPACING))
}

/// Guidance text (extra-small muted text that wraps).
pub fn guidance(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text.into()).color(colors::MUTED).size(layout::TYPE_SM)).wrap())
}

/// An error message in the danger color that wraps.
pub fn error_text(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text.into()).color(colors::DANGER).size(layout::TYPE_SM)).wrap())
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

/// The status text font: the status style, which [`apply_editor`](joy_ui::theme::apply_editor) registers.
fn status_font(ui: &Ui) -> FontId {
    style_font(ui, &text_styles::status(), layout::TYPE_XS)
}

/// The font of `style` in the `Ui`'s style, or the default proportional font at `fallback_size` when the authoring
/// look has not registered it (contexts that never called `apply_editor`, such as some tests), instead of panicking.
fn style_font(ui: &Ui, style: &TextStyle, fallback_size: f32) -> FontId {
    ui.style().text_styles.get(style).cloned().unwrap_or_else(|| FontId::proportional(fallback_size))
}
