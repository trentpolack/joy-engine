// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! egui presentation shared by the editor's panels: the shared Joy tokens (`runtime::theme`), captions in the engine's
//! HUD typography, flat buttons, painted icons, draft-backed text and number fields, panel regions, and resize
//! dividers. Everything here is UI; document rules live in the model modules.

use std::collections::HashMap;
use std::hash::Hash;

use joy_engine::development::components;
use joy_engine::egui::{
    self,
    Align,
    Color32,
    CornerRadius,
    Id,
    Key,
    Modifiers,
    Order,
    Pos2,
    Rect,
    Response,
    RichText,
    Sense,
    Shape,
    Stroke,
    StrokeKind,
    TextEdit,
    Ui,
    UiBuilder,
    Vec2,
    pos2,
    vec2,
};
use joy_engine::runtime::hud_text::{
    self,
    Style,
};
use joy_engine::theme::{
    self,
    colors,
    layout,
};

use joy_engine::development::lab_values::StepScale;

use super::numeric_controls::Draft;

/// Named control rectangles from the latest frame, so tests (and accessibility tooling) can find controls by name.
#[derive(Clone, Debug, Default)]
pub struct ControlMap {
    rects: HashMap<String, Rect>,
}

impl ControlMap {
    /// Forget last frame's controls.
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// Record a control's visible, interactive rectangle (clipped by scroll areas) under `name` and return the
    /// response unchanged.
    pub fn mark(&mut self, name: impl Into<String>, response: Response) -> Response {
        self.rects.insert(name.into(), response.interact_rect);
        response
    }

    /// Record a rectangle under `name`.
    pub fn insert(&mut self, name: impl Into<String>, rect: Rect) {
        self.rects.insert(name.into(), rect);
    }

    /// The rectangle of a named control, in logical points.
    pub fn get(&self, name: &str) -> Option<Rect> {
        self.rects.get(name).copied()
    }

    /// Every recorded name, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.rects.keys().cloned().collect();
        names.sort();
        names
    }
}

/// Painted icons; egui's bundled fonts lack several of the symbols the workspace needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// A circular arrow.
    Refresh,
    /// A funnel.
    Filter,
    /// A trash can.
    Trash,
    /// Three dots.
    More,
    /// A cross.
    Close,
    /// An arrow pointing up and right (opens elsewhere).
    External,
}

/// Show `add_contents` in a fixed window region filled with `fill`.
///
/// Regions are egui areas, so presses inside them belong to the editor and never reach a hosted tool's input; the area
/// between regions stays free for the tool.
pub fn region<R>(
    context: &egui::Context,
    id: impl Hash + std::fmt::Debug,
    rect: Rect,
    fill: Color32,
    order: Order,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    egui::Area::new(Id::new(id))
        .fixed_pos(rect.min)
        .order(order)
        .constrain(false)
        .movable(false)
        .show(context, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            ui.set_clip_rect(rect);
            if fill != Color32::TRANSPARENT {
                ui.painter().rect_filled(rect, 0.0, fill);
            }
            ui.scope_builder(UiBuilder::new().max_rect(rect), add_contents).inner
        })
        .inner
}

/// A thin separator line along one edge of `rect`.
pub fn edge(ui: &Ui, from: Pos2, to: Pos2) {
    ui.painter().line_segment([from, to], Stroke::new(1.0, colors::LINE));
}

/// A small uppercase, letter-spaced caption in the HUD typography (`CONTENT BROWSER`, `TRANSFORM`).
pub fn caption(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let style = Style::new(10.0, color).spacing(1.4).bold();
    let laid_out = hud_text::single(ui.painter(), text, style);
    let (rect, response) = ui.allocate_exact_size(laid_out.size() + vec2(0.0, 2.0), Sense::hover());
    laid_out.paint(ui.painter(), rect.min);
    response
}

/// An accent section heading (`SCENE`, `INSPECTOR`, `COMPONENTS`).
pub fn heading(ui: &mut Ui, text: &str) -> Response {
    caption(ui, text, colors::ACCENT)
}

/// A standard button in the authoring look.
pub fn button(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(label).size(layout::TYPE_SM)))
}

/// The kit's primary action button.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(components::primary_button(label))
}

/// The kit's toggle button, marked while `selected`.
pub fn toggle_button(ui: &mut Ui, label: &str, selected: bool) -> Response {
    components::toggle_button(ui, selected, label)
}

/// The kit's destructive-action button.
pub fn danger_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(components::danger_button(label))
}

/// A square button with a painted icon; `highlight` outlines it in the accent.
pub fn icon_button(ui: &mut Ui, icon: Icon, side: f32, color: Color32, highlight: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let painter = ui.painter();
    let hovered = response.hovered() || response.has_focus();
    let stroke_color = if highlight || hovered { colors::ACCENT } else { colors::LINE };
    painter.rect_filled(rect, 0.0, if hovered { colors::SELECTED_ROW } else { colors::PANEL });
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, stroke_color), StrokeKind::Inside);
    paint_icon(painter, rect.shrink(side * 0.25), icon, if highlight { colors::ACCENT } else { color });
    response
}

/// Paint `icon` inside `rect`.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let (center, size) = (rect.center(), rect.width().min(rect.height()));
    match icon {
        Icon::Refresh => {
            let radius = size * 0.45;
            let points: Vec<Pos2> = (0..=20)
                .map(|step| {
                    let angle = -0.6 + (step as f32 / 20.0) * 5.0;
                    center + (vec2(angle.cos(), angle.sin()) * radius)
                })
                .collect();
            let tip = *points.last().unwrap_or(&center);
            painter.add(Shape::line(points, stroke));
            painter.line_segment([tip, tip + vec2(size * 0.28, -size * 0.05)], stroke);
            painter.line_segment([tip, tip + vec2(size * 0.02, -size * 0.3)], stroke);
        }
        Icon::Filter => {
            let top = rect.top() + (size * 0.1);
            let points = vec![
                pos2(rect.left(), top),
                pos2(rect.right(), top),
                pos2(center.x + (size * 0.12), center.y + (size * 0.05)),
                pos2(center.x + (size * 0.12), rect.bottom()),
                pos2(center.x - (size * 0.12), rect.bottom() - (size * 0.15)),
                pos2(center.x - (size * 0.12), center.y + (size * 0.05)),
            ];
            painter.add(Shape::closed_line(points, stroke));
        }
        Icon::Trash => {
            let left = rect.left() + (size * 0.15);
            let right = rect.right() - (size * 0.15);
            let lid = rect.top() + (size * 0.2);
            painter.line_segment([pos2(rect.left(), lid), pos2(rect.right(), lid)], stroke);
            painter.line_segment(
                [pos2(center.x - (size * 0.15), lid), pos2(center.x - (size * 0.15), rect.top())],
                stroke,
            );
            painter.line_segment(
                [pos2(center.x - (size * 0.15), rect.top()), pos2(center.x + (size * 0.15), rect.top())],
                stroke,
            );
            painter.line_segment(
                [pos2(center.x + (size * 0.15), rect.top()), pos2(center.x + (size * 0.15), lid)],
                stroke,
            );
            let points = vec![
                pos2(left, lid),
                pos2(left + (size * 0.05), rect.bottom()),
                pos2(right - (size * 0.05), rect.bottom()),
                pos2(right, lid),
            ];
            painter.add(Shape::line(points, stroke));
            for offset in [-0.12, 0.12] {
                let x = center.x + (size * offset);
                painter.line_segment([pos2(x, lid + (size * 0.15)), pos2(x, rect.bottom() - (size * 0.12))], stroke);
            }
        }
        Icon::More => {
            for offset in [-0.32, 0.0, 0.32] {
                painter.circle_filled(center + vec2(size * offset, 0.0), size * 0.08, color);
            }
        }
        Icon::Close => {
            let half = size * 0.35;
            painter.line_segment([center + vec2(-half, -half), center + vec2(half, half)], stroke);
            painter.line_segment([center + vec2(-half, half), center + vec2(half, -half)], stroke);
        }
        Icon::External => {
            let half = size * 0.35;
            let tip = center + vec2(half, -half);
            painter.line_segment([center + vec2(-half, half), tip], stroke);
            painter.line_segment([tip, tip + vec2(-half, 0.0)], stroke);
            painter.line_segment([tip, tip + vec2(0.0, half)], stroke);
        }
    }
}

/// What a draft-backed field reported this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldEvent {
    /// New text typed this frame.
    pub changed: Option<String>,
    /// The author pressed Enter or left the field with a draft: commit it.
    pub commit: bool,
    /// The author pressed Escape: discard the draft.
    pub revert: bool,
    /// An arrow-key step: direction (+1 or -1) and modifier.
    pub step: Option<(f64, StepScale)>,
}

/// A single-line field that shows `draft` text while the author edits and `value` otherwise.
///
/// `numeric` fields step with the arrow keys (Shift fine, Alt coarse). An invalid draft draws a danger outline and its
/// message below the field.
pub fn draft_field(
    ui: &mut Ui,
    id: Id,
    value: &str,
    draft: Option<&Draft>,
    width: f32,
    numeric: bool,
    hint: &str,
) -> (Response, FieldEvent) {
    let mut event = FieldEvent::default();
    let focused = ui.memory(|memory| memory.has_focus(id));
    if focused && numeric {
        for (modifiers, modifier) in [
            (Modifiers::NONE, StepScale::Normal),
            (Modifiers::SHIFT, StepScale::Fine),
            (Modifiers::ALT, StepScale::Coarse),
        ] {
            for (key, direction) in [(Key::ArrowUp, 1.0), (Key::ArrowDown, -1.0)] {
                if ui.input_mut(|input| input.consume_key(modifiers, key)) {
                    event.step = Some((direction, modifier));
                }
            }
        }
    }
    let escape = focused && ui.input(|input| input.key_pressed(Key::Escape));
    let mut text = draft.map_or_else(|| value.to_owned(), |draft| draft.text.clone());
    let invalid = draft.is_some_and(|draft| draft.error.is_some());
    let stroke_color = if invalid { colors::DANGER } else { colors::LINE };
    let response = ui
        .scope(|ui| {
            ui.visuals_mut().extreme_bg_color = colors::FIELD;
            ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, stroke_color);
            ui.visuals_mut().widgets.hovered.bg_stroke =
                Stroke::new(1.0, if invalid { colors::DANGER } else { colors::ACCENT });
            TextEdit::singleline(&mut text)
                .id(id)
                .hint_text(hint)
                .desired_width(width)
                .margin(vec2(6.0, 4.0))
                .horizontal_align(Align::LEFT)
                .show(ui)
                .response
                .response
        })
        .inner;
    if response.changed() {
        event.changed = Some(text);
    }
    if response.lost_focus() {
        if escape {
            event.revert = true;
        } else if draft.is_some() || event.changed.is_some() {
            event.commit = true;
        }
    }
    // The message rides on the field (and the status line) so rows of fields keep their layout.
    let response = match draft.and_then(|draft| draft.error.as_deref()) {
        Some(error) => response.on_hover_text(RichText::new(error).color(colors::DANGER)),
        None => response,
    };
    (response, event)
}

/// A vertical resize handle at `rect`; returns the horizontal drag in points and whether the drag ended. Arrow keys
/// move a focused handle by `key_step`.
pub fn divider(ui: &mut Ui, id: Id, rect: Rect, key_step: f32) -> (f32, bool) {
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let active = response.hovered() || response.dragged() || response.has_focus();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let color = if active { colors::ACCENT } else { colors::LINE };
    ui.painter()
        .line_segment([rect.center_top(), rect.center_bottom()], Stroke::new(if active { 2.0 } else { 1.0 }, color));
    if response.clicked() {
        response.request_focus();
    }
    let mut delta = response.drag_delta().x;
    if response.has_focus() {
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowLeft)) {
            delta -= key_step;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowRight)) {
            delta += key_step;
        }
    }
    (delta, response.drag_stopped() || (response.has_focus() && delta != 0.0))
}

/// Paint a small square badge with centered text (asset kind marks such as `FX` and `L`).
pub fn badge(painter: &egui::Painter, rect: Rect, text: &str, fill: Color32, color: Color32) {
    painter.rect_filled(rect, 0.0, fill);
    let laid_out = hud_text::single(painter, text, Style::new(9.0, color).bold());
    laid_out.paint_in(painter, rect, Align::Center, Align::Center);
}

/// The theme's standard overlay frame for dialogs.
pub fn dialog_frame() -> egui::Frame {
    theme::panel_frame()
        .fill(colors::PANEL)
        .stroke(Stroke::new(layout::BORDER_THIN, colors::ACCENT))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(egui::Margin::same(layout::SPACE_XL as i8))
}
