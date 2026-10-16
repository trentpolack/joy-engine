// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The demo's HUD, matching the TypeScript page (`site/index.html`, `style.css`): title and view buttons, the brief with
//! seed, backend, chunk, and debris readouts, the integrity or support legend with the last chunk's resistance, the
//! CONFIG panel, the notice line, and the footer with RESET. Information panels ignore the pointer so clicks reach the
//! structure behind them; drawing returns the requested changes instead of mutating the demo.

use joy_engine::development::dock;
use joy_engine::egui::{Align2, Color32, FontId, Id, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2 as UiVec2};
use joy_engine::prelude::*;

use super::config::destruction_config::StructureView;

/// Configurable ranges and steps (TypeScript `index.html`).
pub const RADIUS_RANGE: (f64, f64, f64) = (0.6, 2.0, 0.05);
/// Damage-per-impact range and step.
pub const DAMAGE_RANGE: (f64, f64, f64) = (0.5, 3.0, 0.5);

/// Viewport widths at most this wide use the compact layout, and at most [`TINY_WIDTH`] the smallest one.
const COMPACT_WIDTH: f32 = 760.0;
const TINY_WIDTH: f32 = 480.0;
/// Viewports at most this tall hide the brief's paragraph.
const SHORT_HEIGHT: f32 = 700.0;
/// Footer bar height.
const FOOTER_HEIGHT: f32 = 48.0;

// The page's palette.
const ORANGE: Color32 = Color32::from_rgb(0xff, 0x64, 0x1a);
const BLUE: Color32 = Color32::from_rgb(0x58, 0xaf, 0xdf);
const PRESSED_TEXT: Color32 = Color32::from_rgb(0xa8, 0xd9, 0xf5);
const PANEL: Color32 = Color32::from_rgba_premultiplied(0x0e, 0x13, 0x18, 0xe8);
const TEXT: Color32 = Color32::from_rgb(0xee, 0xf2, 0xf4);
const BUTTON_TEXT: Color32 = Color32::from_rgb(0xf7, 0xf8, 0xf9);
const MUTED: Color32 = Color32::from_rgb(0xae, 0xb8, 0xbf);
const LABEL: Color32 = Color32::from_rgb(0xc4, 0xce, 0xd5);
const RULE: Color32 = Color32::from_rgb(0x30, 0x39, 0x42);
const BUTTON: Color32 = Color32::from_rgb(0x15, 0x1b, 0x21);
const BUTTON_LINE: Color32 = Color32::from_rgb(0x48, 0x51, 0x5a);
const FOOTER: Color32 = Color32::from_rgba_premultiplied(0x0b, 0x0f, 0x14, 0xe8);
const FOOTER_LINE: Color32 = Color32::from_rgb(0x29, 0x30, 0x39);
const FOOTER_TEXT: Color32 = Color32::from_rgb(0x96, 0xa0, 0xa8);
const NOTICE: Color32 = Color32::from_rgba_premultiplied(0x06, 0x09, 0x0b, 0xcc);
const NOTICE_TEXT: Color32 = Color32::from_rgb(0xd5, 0xda, 0xdd);
const METER: Color32 = Color32::from_rgb(0x4c, 0xb0, 0x50);

/// Integrity and support legend gradients: stop position and color.
const INTEGRITY_SCALE: [(f32, Color32); 4] = [
    (0.0, Color32::from_rgb(0xff, 0x34, 0x25)),
    (0.3, Color32::from_rgb(0xfa, 0xca, 0x39)),
    (0.6, Color32::from_rgb(0x4b, 0xba, 0xd8)),
    (1.0, Color32::from_rgb(0x61, 0x6c, 0xdc)),
];
const SUPPORT_SCALE: [(f32, Color32); 3] = [
    (0.0, Color32::from_rgb(0x4b, 0xba, 0xd8)),
    (0.5, Color32::from_rgb(0x43, 0xc2, 0x72)),
    (1.0, Color32::from_rgb(0xfa, 0xca, 0x39)),
];

/// Everything the HUD shows, refreshed by the demo after changes and every few frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudState {
    /// Graphics backend label.
    pub backend: String,
    /// Structure seed.
    pub seed: i64,
    /// Active chunk readout.
    pub chunks: String,
    /// Retained debris readout.
    pub debris: String,
    /// Hits to break intact masonry and frame chunks at the current damage.
    pub baseline: String,
    /// The last chunk's support path.
    pub support: String,
    /// The last chunk's remaining and maximum resistance.
    pub meter: (f32, f32),
    /// The last chunk's resistance summary.
    pub target: String,
    /// The latest action's outcome.
    pub notice: String,
}

/// A HUD control's requested change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HudCommand {
    /// Toggle between the integrity and material views.
    ToggleIntegrity,
    /// Toggle between the support and material views.
    ToggleSupport,
    /// Open or close CONFIG.
    ToggleConfig,
    /// Close CONFIG.
    CloseConfig,
    /// Set the impact radius in meters.
    Radius(f64),
    /// Set the damage per impact.
    Damage(f64),
    /// Show or hide impact markers.
    ImpactDebug(bool),
    /// Restore the structure and clear debris.
    Reset,
}

/// The demo settings the HUD's controls edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudSettings {
    /// Current structure view.
    pub view: StructureView,
    /// Whether CONFIG is open.
    pub config_open: bool,
    /// Impact radius in meters.
    pub damage_radius: f64,
    /// Damage per impact.
    pub damage_per_hit: f64,
    /// Whether impact markers draw.
    pub impact_debug: bool,
}

/// Draw the HUD and return the requested changes.
pub fn draw(context: &egui::Context, state: &HudState, settings: HudSettings) -> Vec<HudCommand> {
    let screen = context.content_rect();
    let layout = Layout {
        compact: screen.width() <= COMPACT_WIDTH,
        tiny: screen.width() <= TINY_WIDTH,
        short: screen.height() <= SHORT_HEIGHT,
    };
    let footer_bottom = dock::reserved_height(context);
    let mut commands = Vec::new();
    header(context, layout, settings, &mut commands);
    brief(context, layout, state);
    if settings.view != StructureView::Material {
        legend(context, layout, state, settings.view, footer_bottom);
    }
    if settings.config_open {
        config(context, layout, settings, &mut commands);
    }
    notice(context, layout, state, footer_bottom);
    footer(context, layout, footer_bottom, &mut commands);
    commands
}

/// Responsive breakpoints from the page's media queries.
#[derive(Clone, Copy, Debug)]
struct Layout {
    compact: bool,
    tiny: bool,
    short: bool,
}

fn header(context: &egui::Context, layout: Layout, settings: HudSettings, commands: &mut Vec<HudCommand>) {
    let padding = if layout.compact { UiVec2::new(14.0, 14.0) } else { UiVec2::new(25.0, 22.0) };
    let title_size = if layout.tiny {
        15.0
    } else if layout.compact {
        18.0
    } else {
        23.0
    };
    egui::Area::new(Id::new("destruction-title")).anchor(Align2::LEFT_TOP, padding).interactable(false).show(
        context,
        |ui| {
            ui.label(RichText::new("JOY ENGINE / EXAMPLE").size(10.0).strong().color(ORANGE));
            ui.label(RichText::new("FORM DESTRUCTION").font(FontId::proportional(title_size)).strong().color(TEXT));
        },
    );
    egui::Area::new(Id::new("destruction-nav")).anchor(Align2::RIGHT_TOP, padding * UiVec2::new(-1.0, 1.0)).show(
        context,
        |ui| {
            let integrity = settings.view == StructureView::Integrity;
            let support = settings.view == StructureView::Support;
            let buttons = [
                (if integrity { "INTEGRITY ON" } else { "INTEGRITY" }, integrity, HudCommand::ToggleIntegrity),
                (if support { "SUPPORT ON" } else { "SUPPORT" }, support, HudCommand::ToggleSupport),
                ("CONFIG", settings.config_open, HudCommand::ToggleConfig),
            ];
            let mut draw = |ui: &mut Ui| {
                for (label, pressed, command) in buttons {
                    if button(ui, RichText::new(label), pressed && command != HudCommand::ToggleConfig, layout.tiny) {
                        commands.push(command);
                    }
                }
            };
            if layout.tiny {
                ui.vertical(|ui| draw(ui));
            } else {
                ui.horizontal(|ui| draw(ui));
            }
        },
    );
}

fn brief(context: &egui::Context, layout: Layout, state: &HudState) {
    let (offset, width) = if layout.tiny {
        (UiVec2::new(10.0, 84.0), 175.0)
    } else if layout.compact {
        (UiVec2::new(10.0, 84.0), 200.0)
    } else {
        (UiVec2::new(24.0, 102.0), 235.0)
    };
    egui::Area::new(Id::new("destruction-brief")).anchor(Align2::LEFT_TOP, offset).interactable(false).show(
        context,
        |ui| {
            accent_panel(ui, ORANGE, width, |ui| {
                ui.label(RichText::new("CLICK TO APPLY AN IMPACT").size(12.0).strong().color(TEXT));
                if !layout.compact && !layout.short {
                    ui.label(
                        RichText::new(
                            "Impacts weaken the wall. Removing its supports makes overhangs fall, even with health \
                             remaining. Debris stays until reset.",
                        )
                        .size(12.0)
                        .color(MUTED),
                    );
                }
                ui.add_space(6.0);
                for (label, value) in [
                    ("SEED", state.seed.to_string()),
                    ("BACKEND", state.backend.to_uppercase()),
                    ("CHUNKS", state.chunks.clone()),
                    ("DEBRIS", state.debris.clone()),
                ] {
                    rule(ui);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).size(10.0).strong().color(ORANGE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(value).size(10.0).strong().color(TEXT));
                        });
                    });
                }
            });
        },
    );
}

fn legend(context: &egui::Context, layout: Layout, state: &HudState, view: StructureView, footer_bottom: f32) {
    let bottom = if layout.compact {
        105.0
    } else if layout.short {
        75.0
    } else {
        84.0
    };
    let (left, width) = if layout.compact { (10.0, 260.0) } else { (24.0, 290.0) };
    let support = view == StructureView::Support;
    egui::Area::new(Id::new("destruction-legend"))
        .anchor(Align2::LEFT_BOTTOM, UiVec2::new(left, -bottom - footer_bottom))
        .interactable(false)
        .show(context, |ui| {
            accent_panel(ui, BLUE, width, |ui| {
                let title = if support { "SUPPORT / GROUND CONNECTION" } else { "INTEGRITY / IMPACT RESISTANCE" };
                ui.label(RichText::new(title).size(11.0).strong().color(TEXT));
                if support {
                    gradient(ui, &SUPPORT_SCALE);
                } else {
                    gradient(ui, &INTEGRITY_SCALE);
                }
                let labels: &[&str] =
                    if support { &["ANCHORED", "VERTICAL", "BRACED"] } else { &["CRITICAL", "WEAK", "SOLID", "FRAME"] };
                ui.columns(labels.len(), |columns| {
                    let last = labels.len() - 1;
                    for (index, (column, label)) in columns.iter_mut().zip(labels).enumerate() {
                        let align = match index {
                            0 => egui::Align::Min,
                            index if index == last => egui::Align::Max,
                            _ => egui::Align::Center,
                        };
                        column.with_layout(egui::Layout::top_down(align), |ui| {
                            ui.label(RichText::new(*label).size(8.0).color(LABEL));
                        });
                    }
                });
                ui.label(RichText::new(&state.baseline).size(11.0).color(LABEL));
                rule(ui);
                ui.label(RichText::new(&state.support).size(11.0).color(LABEL));
                let (value, maximum) = state.meter;
                let fraction = if maximum > 0.0 { (value / maximum).clamp(0.0, 1.0) } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).fill(METER).desired_height(10.0).corner_radius(5.0));
                ui.label(RichText::new(&state.target).size(11.0).color(TEXT));
            });
        });
}

fn config(context: &egui::Context, layout: Layout, settings: HudSettings, commands: &mut Vec<HudCommand>) {
    let right = if layout.compact { 14.0 } else { 24.0 };
    let width = (context.content_rect().width() - (right * 2.0)).min(280.0);
    egui::Area::new(Id::new("destruction-config")).anchor(Align2::RIGHT_TOP, UiVec2::new(-right, 74.0)).show(
        context,
        |ui| {
            egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, ORANGE)).inner_margin(Margin::same(18)).show(
                ui,
                |ui| {
                    ui.set_width(width - 36.0);
                    ui.visuals_mut().selection.bg_fill = ORANGE;
                    ui.visuals_mut().override_text_color = Some(LABEL);
                    ui.label(RichText::new("DESTRUCTION CONFIG").size(13.0).strong().color(TEXT));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Impact radius").size(12.0));
                    let mut radius = settings.damage_radius;
                    let (minimum, maximum, step) = RADIUS_RANGE;
                    if ui.add(egui::Slider::new(&mut radius, minimum..=maximum).step_by(step)).changed() {
                        commands.push(HudCommand::Radius(radius));
                    }
                    ui.label(RichText::new("Damage per impact").size(12.0));
                    let mut damage = settings.damage_per_hit;
                    let (minimum, maximum, step) = DAMAGE_RANGE;
                    if ui.add(egui::Slider::new(&mut damage, minimum..=maximum).step_by(step)).changed() {
                        commands.push(HudCommand::Damage(damage));
                    }
                    let mut debug = settings.impact_debug;
                    if ui.checkbox(&mut debug, RichText::new("Impact debug draws").size(12.0)).changed() {
                        commands.push(HudCommand::ImpactDebug(debug));
                    }
                    ui.label(
                        RichText::new("White: hit point · Yellow: incoming ray · Cyan: damage radius")
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.add_space(6.0);
                    if button(ui, RichText::new("CLOSE"), false, false) {
                        commands.push(HudCommand::CloseConfig);
                    }
                },
            );
        },
    );
}

fn notice(context: &egui::Context, layout: Layout, state: &HudState, footer_bottom: f32) {
    let bottom = if layout.compact { FOOTER_HEIGHT + 30.0 } else { FOOTER_HEIGHT + 20.0 };
    egui::Area::new(Id::new("destruction-notice"))
        .anchor(Align2::CENTER_BOTTOM, UiVec2::new(0.0, -bottom - footer_bottom))
        .interactable(false)
        .show(context, |ui| {
            egui::Frame::new().fill(NOTICE).inner_margin(Margin::symmetric(11, 7)).show(ui, |ui| {
                ui.label(RichText::new(&state.notice).size(12.0).color(NOTICE_TEXT));
            });
        });
}

fn footer(context: &egui::Context, layout: Layout, footer_bottom: f32, commands: &mut Vec<HudCommand>) {
    let width = context.content_rect().width();
    egui::Area::new(Id::new("destruction-footer"))
        .anchor(Align2::LEFT_BOTTOM, UiVec2::new(0.0, -footer_bottom))
        .constrain(false)
        .show(context, |ui| {
            let padding: i8 = if layout.compact { 12 } else { 24 };
            egui::Frame::new().fill(FOOTER).inner_margin(Margin::symmetric(padding, 8)).show(ui, |ui| {
                ui.set_width(width - (f32::from(padding) * 2.0));
                ui.set_height(FOOTER_HEIGHT - 16.0);
                let top = ui.max_rect().top() - 8.0;
                ui.painter().hline(
                    ui.max_rect().x_range().expand(f32::from(padding)),
                    top,
                    Stroke::new(1.0, FOOTER_LINE),
                );
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = if layout.compact { 12.0 } else { 26.0 };
                    if !layout.tiny {
                        for hint in ["CLICK IMPACT", "DRAG ORBIT", "SCROLL ZOOM"] {
                            ui.label(RichText::new(hint).size(10.0).strong().color(FOOTER_TEXT));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = (
                            RichText::new("RESET").size(11.0).strong(),
                            RichText::new("R").size(11.0).strong().color(ORANGE),
                        );
                        if styled_button(ui, label, false, false) {
                            commands.push(HudCommand::Reset);
                        }
                    });
                });
            });
        });
}

/// A square page button; `pressed` buttons take the blue toggled look.
fn button(ui: &mut Ui, label: RichText, pressed: bool, small: bool) -> bool {
    styled_button(ui, label, pressed, small)
}

fn styled_button<'a>(ui: &mut Ui, atoms: impl egui::IntoAtoms<'a>, pressed: bool, small: bool) -> bool {
    let (line, text) = if pressed { (BLUE, PRESSED_TEXT) } else { (BUTTON_LINE, BUTTON_TEXT) };
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        visuals.override_text_color = Some(text);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, ORANGE);
        let padding = if small { UiVec2::new(9.0, 7.0) } else { UiVec2::new(12.0, 9.0) };
        ui.spacing_mut().button_padding = padding;
        let response = ui.add(egui::Button::new(atoms).fill(BUTTON).stroke(Stroke::new(1.0, line)).corner_radius(0.0));
        if response.hovered() {
            ui.painter().rect_stroke(response.rect, 0.0, Stroke::new(1.0, ORANGE), egui::StrokeKind::Inside);
        }
        response.clicked()
    })
    .inner
}

/// A translucent panel with a three-pixel accent along its left edge.
fn accent_panel(ui: &mut Ui, accent: Color32, width: f32, add_contents: impl FnOnce(&mut Ui)) {
    let response = egui::Frame::new()
        .fill(PANEL)
        .inner_margin(Margin { left: 18, right: 15, top: 14, bottom: 14 })
        .show(ui, |ui| {
            ui.set_width(width - 33.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            add_contents(ui);
        });
    let rect = response.response.rect;
    ui.painter().rect_filled(Rect::from_min_size(rect.min, UiVec2::new(3.0, rect.height())), 0.0, accent);
}

/// A thin separator in the page's rule color.
fn rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(UiVec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, RULE);
}

/// A seven-pixel horizontal gradient through `stops`.
fn gradient(ui: &mut Ui, stops: &[(f32, Color32)]) {
    let (rect, _) = ui.allocate_exact_size(UiVec2::new(ui.available_width(), 7.0), Sense::hover());
    const STEPS: usize = 64;
    let step_width = rect.width() / STEPS as f32;
    for step in 0..STEPS {
        let position = (step as f32 + 0.5) / STEPS as f32;
        let upper = stops.iter().position(|(stop, _)| *stop >= position).unwrap_or(stops.len() - 1).max(1);
        let (start, from) = stops[upper - 1];
        let (end, to) = stops[upper];
        let blend = ((position - start) / (end - start).max(1e-6)).clamp(0.0, 1.0);
        let color = lerp_color(from, to, blend);
        let left = rect.left() + (step as f32 * step_width);
        let cell = Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(left + step_width + 0.5, rect.bottom()));
        ui.painter().rect_filled(cell, 0.0, color);
    }
}

/// Linear blend between two opaque colors in display space, as CSS gradients interpolate.
fn lerp_color(from: Color32, to: Color32, blend: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + ((f32::from(b) - f32::from(a)) * blend)).round() as u8;
    Color32::from_rgb(channel(from.r(), to.r()), channel(from.g(), to.g()), channel(from.b(), to.b()))
}
