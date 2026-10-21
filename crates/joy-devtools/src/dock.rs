// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The development dock: a row of launchers at the bottom-right corner whose panels open above it, one at a time.
//!
//! Tools that are present this frame take slots from the right in a fixed order: post-processing, config, then stats.
//! Opening one panel closes the others. Tools may claim keys (for example a pause shortcut) so the shell keeps them
//! from gameplay.

use egui::{
    Align2,
    Color32,
    Context,
    Id,
    RichText,
    Stroke,
    Ui,
    Vec2,
};
use joy_core::math::deterministic;
use joy_ui::theme::tokens;

// Development widgets read the shared design tokens; only their darker overlay surfaces differ from authoring panels,
// so runtime tools stay legible over bright game frames.
/// Development widget accent.
pub const ACCENT: Color32 = tokens::ACCENT;
/// Development widget panel fill.
pub const BACKGROUND: Color32 = tokens::OVERLAY;
/// Development widget input fill.
pub const INPUT: Color32 = tokens::OVERLAY_FIELD;
/// Development widget text.
pub const TEXT: Color32 = tokens::TEXT;
/// Development widget secondary text.
pub const MUTED: Color32 = tokens::MUTED;
/// Development widget separators and input borders.
pub const LINE: Color32 = tokens::LINE;
/// Development widget error text.
pub const ERROR: Color32 = tokens::DANGER;

/// Distance from the viewport edges in logical pixels.
const EDGE: f32 = 12.0;
/// Launcher height in logical pixels.
const LAUNCHER_HEIGHT: f32 = 36.0;
/// Widest launcher in logical pixels.
const LAUNCHER_WIDTH: f32 = 136.0;
/// Gap between launchers, and between the row and an open panel.
const GAP: f32 = 8.0;
/// Widest panel in logical pixels.
const PANEL_WIDTH: f32 = 360.0;
/// Tallest panel in logical pixels.
const PANEL_HEIGHT: f32 = 720.0;

/// A development tool with a launcher in the dock, in slot order from the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockTool {
    /// The post-processing editor.
    PostProcessing,
    /// A game's configuration panel.
    Config,
    /// Runtime statistics.
    Stats,
}

impl DockTool {
    const ALL: [DockTool; 3] = [DockTool::PostProcessing, DockTool::Config, DockTool::Stats];

    fn presence_id(self) -> Id {
        Id::new(("joy-dev-dock-present", self))
    }
}

fn open_id() -> Id {
    Id::new("joy-dev-dock-open")
}

fn claimed_keys_id() -> Id {
    Id::new("joy-dev-dock-claimed-keys")
}

/// Whether a tool registered a launcher this pass or the previous one.
fn present(context: &Context, tool: DockTool) -> bool {
    let pass = context.cumulative_pass_nr();
    context.data(|data| data.get_temp::<u64>(tool.presence_id())).is_some_and(|seen| seen + 1 >= pass)
}

/// Launcher width that fits `count` launchers on narrow viewports.
fn launcher_width(context: &Context, count: usize) -> f32 {
    let count = count.max(1) as f32;
    let available = context.content_rect().width() - (EDGE * 2.0) - ((count - 1.0) * GAP);
    LAUNCHER_WIDTH.min(available / count).max(48.0)
}

/// Height the dock occupies above the bottom edge, so HUD elements in the bottom-right corner can sit above it.
pub fn reserved_height(context: &Context) -> f32 {
    if DockTool::ALL.iter().any(|tool| present(context, *tool)) { EDGE + LAUNCHER_HEIGHT + GAP } else { 0.0 }
}

/// The tool whose panel is open, if any.
pub fn open_tool(context: &Context) -> Option<DockTool> {
    context.data(|data| data.get_temp::<Option<DockTool>>(open_id())).flatten()
}

/// Open or close a tool's panel; opening one closes any other.
pub fn set_open(context: &Context, tool: DockTool, open: bool) {
    context.data_mut(|data| {
        let current = data.get_temp::<Option<DockTool>>(open_id()).flatten();
        if open {
            data.insert_temp(open_id(), Some(tool));
        } else if current == Some(tool) {
            data.insert_temp(open_id(), None::<DockTool>);
        }
    });
}

/// Close whichever panel is open; returns whether one was. The shell calls this for Escape before gameplay sees it.
pub fn close_open_panel(context: &Context) -> bool {
    let open = open_tool(context).is_some();
    context.data_mut(|data| data.insert_temp(open_id(), None::<DockTool>));
    open
}

/// Keep a DOM key code (for example `"KeyP"`) from reaching gameplay this frame and the next.
pub fn claim_key(context: &Context, code: &str) {
    let pass = context.cumulative_pass_nr();
    context.data_mut(|data| {
        let mut claimed: Vec<(String, u64)> = data.get_temp(claimed_keys_id()).unwrap_or_default();
        claimed.retain(|(existing, seen)| existing != code && seen + 1 >= pass);
        claimed.push((code.to_owned(), pass));
        data.insert_temp(claimed_keys_id(), claimed);
    });
}

/// Whether a development tool claimed a key code.
pub fn key_claimed(context: &Context, code: &str) -> bool {
    let pass = context.cumulative_pass_nr();
    context
        .data(|data| data.get_temp::<Vec<(String, u64)>>(claimed_keys_id()))
        .is_some_and(|claimed| claimed.iter().any(|(existing, seen)| existing == code && seen + 1 >= pass))
}

/// Draw a tool's launcher in its slot and toggle its panel when clicked. Returns whether the panel is open.
pub fn launcher(context: &Context, tool: DockTool, label: RichText) -> bool {
    let pass = context.cumulative_pass_nr();
    context.data_mut(|data| data.insert_temp(tool.presence_id(), pass));
    let present: Vec<DockTool> = DockTool::ALL.into_iter().filter(|candidate| present(context, *candidate)).collect();
    let slot = present.iter().position(|candidate| *candidate == tool).unwrap_or(0) as f32;
    let width = launcher_width(context, present.len());
    let offset = Vec2::new(-EDGE - (slot * (width + GAP)), -EDGE);
    let open = open_tool(context) == Some(tool);

    egui::Area::new(Id::new(("joy-dev-dock-launcher", tool)))
        .anchor(Align2::RIGHT_BOTTOM, offset)
        .order(egui::Order::Foreground)
        .show(context, |ui| {
            let fill = if open { ACCENT } else { BACKGROUND };
            let color = if open { BACKGROUND } else { ACCENT };
            let button = egui::Button::new(label.color(color).monospace().strong())
                .fill(fill)
                .stroke(Stroke::new(1.0, ACCENT))
                .corner_radius(0.0)
                .min_size(Vec2::new(width, LAUNCHER_HEIGHT));
            if ui.add_sized(Vec2::new(width, LAUNCHER_HEIGHT), button).clicked() {
                set_open(context, tool, !open);
            }
        });
    open_tool(context) == Some(tool)
}

/// Draw a tool's panel above the dock when it is open. `title` heads the panel beside a close button.
pub fn panel(context: &Context, tool: DockTool, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    if open_tool(context) != Some(tool) {
        return;
    }
    let screen = context.content_rect();
    let width = PANEL_WIDTH.min(screen.width() - (EDGE * 2.0));
    let bottom = EDGE + LAUNCHER_HEIGHT + GAP;
    let height = PANEL_HEIGHT.min(screen.height() - bottom - EDGE);
    egui::Area::new(Id::new(("joy-dev-dock-panel", tool)))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-EDGE, -bottom))
        .order(egui::Order::Foreground)
        .show(context, |ui| {
            egui::Frame::new()
                .fill(BACKGROUND)
                .stroke(Stroke::new(1.0, ACCENT))
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(width - 30.0);
                    style(ui);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).monospace().strong().color(TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new("×").monospace()).on_hover_text("Close").clicked() {
                                set_open(context, tool, false);
                            }
                        });
                    });
                    egui::ScrollArea::vertical()
                        .max_height(height - 64.0)
                        .auto_shrink([false, true])
                        .show(ui, add_contents);
                });
        });
}

/// Apply the development widget look to a panel's contents.
fn style(ui: &mut Ui) {
    let visuals = &mut ui.style_mut().visuals;
    visuals.override_text_color = Some(TEXT);
    visuals.extreme_bg_color = INPUT;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, BACKGROUND);
    for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.noninteractive] {
        widget.bg_stroke = Stroke::new(1.0, LINE);
        widget.corner_radius = 0.0.into();
    }
    visuals.widgets.inactive.weak_bg_fill = BACKGROUND;
    visuals.widgets.inactive.bg_fill = INPUT;
    for widget in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        widget.bg_stroke = Stroke::new(1.0, ACCENT);
        widget.corner_radius = 0.0.into();
    }
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.slider_trailing_fill = true;
    ui.style_mut().override_font_id = Some(egui::FontId::monospace(12.0));
}

/// A muted explanatory note.
pub fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(MUTED));
}

/// A group heading in the widget style.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text.to_uppercase()).strong().color(ACCENT));
}

/// Decimal places that show a step exactly (0.01 → 2).
pub(crate) fn step_decimals(step: f64) -> usize {
    let mut places = 0;
    let mut scaled = step;
    while places < 6 && (scaled - scaled.round()).abs() > 1e-9 {
        scaled *= 10.0;
        places += 1;
    }
    places
}

/// Snap a value to its control's step so drags store authored-looking numbers.
pub(crate) fn round_to_step(value: f64, step: f64) -> f64 {
    let places = step_decimals(step) as i32;
    let scale = deterministic::powi(10_f64, places);
    (value * scale).round() / scale
}

#[cfg(test)]
#[path = "dock.test.rs"]
mod tests;
