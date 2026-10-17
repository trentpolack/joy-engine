// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The example's sidebar: what the two bins compare, live pile and contact diagnostics per bin, the colliders' build
//! costs, the benchmark result, and DROP MORE, PAUSE, and RESET. Drawing returns the requested changes instead of
//! mutating the demo.

use joy_engine::development::dock;
use joy_engine::egui::{
    Align2,
    Color32,
    FontId,
    Id,
    Margin,
    RichText,
    Stroke,
    Vec2 as UiVec2,
};
use joy_engine::prelude::*;

use super::config::sdf_collisions_config::PRESENTATION;
use super::pile_arena::ArenaSnapshot;
use super::prop_library::Prop;

/// Sidebar width in logical pixels, and on viewports at most [`NARROW_VIEWPORT`] wide.
pub const SIDEBAR_WIDTH: f32 = 330.0;
const SIDEBAR_WIDTH_NARROW: f32 = 260.0;
const NARROW_VIEWPORT: f32 = 760.0;

// The sidebar palette.
const FILL: Color32 = Color32::from_rgba_premultiplied(0x0d, 0x14, 0x1a, 0xf0);
const TITLE: Color32 = Color32::from_rgb(0x8f, 0xe3, 0xd2);
const TEXT: Color32 = Color32::from_rgb(0xe3, 0xea, 0xee);
const MUTED: Color32 = Color32::from_rgb(0x9d, 0xab, 0xb5);
const RULE: Color32 = Color32::from_rgb(0x2b, 0x36, 0x40);
const BUTTON: Color32 = Color32::from_rgb(0x1a, 0x25, 0x2e);
const BUTTON_LINE: Color32 = Color32::from_rgb(0x46, 0x56, 0x63);
const WARNING: Color32 = Color32::from_rgb(0xff, 0xb0, 0x8a);

/// Fields smaller than this are analytical: they store dimensions, not a grid.
const ANALYTICAL_FIELD_BYTES: usize = 4096;

/// A change requested from the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudCommand {
    /// Grow both piles by a burst of props.
    DropMore,
    /// Pause or resume both simulations.
    TogglePause,
    /// Empty both bins and start the drop sequence again.
    Reset,
}

/// Everything the sidebar shows, borrowed from the demo for one frame.
pub struct HudView<'a> {
    pub sdf: &'a ArenaSnapshot,
    pub boxes: &'a ArenaSnapshot,
    pub props: &'a [Prop],
    pub field_bytes: usize,
    pub paused: bool,
    pub backend: &'a str,
    pub render_milliseconds: f64,
    pub benchmark: Option<&'a str>,
}

/// Sidebar width for a viewport width in logical pixels.
pub fn sidebar_width(viewport_width: f32) -> f32 {
    if viewport_width <= NARROW_VIEWPORT { SIDEBAR_WIDTH_NARROW } else { SIDEBAR_WIDTH }
}

/// Draw the sidebar on the right edge and return the commands it requested.
pub fn draw(context: &egui::Context, view: &HudView<'_>) -> Vec<HudCommand> {
    let mut commands = Vec::new();
    let screen = context.content_rect();
    let width = sidebar_width(screen.width());
    let margin: i8 = if width == SIDEBAR_WIDTH { 20 } else { 12 };
    let reserved = dock::reserved_height(context);
    egui::Area::new(Id::new("sdf-collisions-sidebar")).anchor(Align2::RIGHT_TOP, UiVec2::ZERO).constrain(false).show(
        context,
        |ui| {
            egui::Frame::new().fill(FILL).inner_margin(Margin::same(margin)).show(ui, |ui| {
                let inner = UiVec2::new(width, screen.height()) - UiVec2::splat(f32::from(margin) * 2.0);
                ui.set_width(inner.x);
                ui.set_height(inner.y);
                style(ui);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    contents(ui, view, &mut commands);
                    ui.add_space(reserved);
                });
            });
        },
    );
    commands
}

fn contents(ui: &mut egui::Ui, view: &HudView<'_>, commands: &mut Vec<HudCommand>) {
    ui.label(RichText::new("SDF COLLISIONS").font(FontId::proportional(20.0)).strong().color(TITLE));
    ui.label(
        RichText::new("The same FORM props, dropped in the same poses. Left: signed distance colliders. Right: the boxes the engine could use before.")
            .color(MUTED),
    );
    ui.label(
        RichText::new("Drag to orbit · scroll to zoom · Space drops more · P pauses · R resets").small().color(MUTED),
    );

    section(ui, "SDF COLLIDERS", PRESENTATION.sdf_tint);
    bin_lines(ui, view.sdf);
    let statistics = view.sdf.statistics;
    ui.label(format!("{} pairs · {} contacts", statistics.pairs, statistics.contacts));
    ui.label(format!("{} iterations · {} distance queries", statistics.iterations, statistics.distance_queries));

    section(ui, "BOUNDING-BOX COLLIDERS", PRESENTATION.box_tint);
    bin_lines(ui, view.boxes);

    section(ui, "COLLIDERS", PRESENTATION.wall_tint);
    ui.label(
        RichText::new(format!("Distance grids share {:.1} MB across every body.", megabytes(view.field_bytes)))
            .small()
            .color(MUTED),
    );
    for prop in view.props {
        let field = if prop.field_bytes >= ANALYTICAL_FIELD_BYTES {
            format!("{} triangles · {:.1} MB grid", prop.triangles, megabytes(prop.field_bytes))
        } else {
            "analytical torus".to_owned()
        };
        ui.label(format!(
            "{}: {field} · {:.0} ms · {} seeds",
            prop.name,
            prop.collider_milliseconds,
            prop.shape.seed_count()
        ));
    }

    ui.add_space(6.0);
    let full = |ui: &mut egui::Ui, label: &str| {
        ui.add(egui::Button::new(label).min_size(UiVec2::new(ui.available_width(), 28.0))).clicked()
    };
    if full(ui, "DROP MORE") {
        commands.push(HudCommand::DropMore);
    }
    if full(ui, if view.paused { "RESUME" } else { "PAUSE" }) {
        commands.push(HudCommand::TogglePause);
    }
    if full(ui, "RESET") {
        commands.push(HudCommand::Reset);
    }

    ui.add_space(6.0);
    ui.label(
        RichText::new(format!("{} · render encode {:.2} ms (CPU)", view.backend, view.render_milliseconds))
            .small()
            .color(MUTED),
    );
    if let Some(benchmark) = view.benchmark {
        ui.label(RichText::new(benchmark).small().monospace().color(TEXT));
    }
    ui.label(
        RichText::new("Contacts: Giles and Andrews, Real-Time Collision Handling for Signed Distance Fields via Caching and Importance Sampling (Pacific Graphics 2026).")
            .small()
            .color(MUTED),
    );
}

/// Bytes to megabytes for display.
fn megabytes(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// A section heading with a color swatch matching the bin floor.
fn section(ui: &mut egui::Ui, title: &str, tint: Rgb) {
    ui.add_space(4.0);
    ui.separator();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(UiVec2::splat(12.0), egui::Sense::hover());
        let [red, green, blue] = tint.map(|channel| (channel.clamp(0.0, 1.0) * 255.0) as u8);
        ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(red, green, blue));
        ui.label(RichText::new(title).strong().color(TEXT));
    });
}

/// Pile size, escapes, and physics time for one bin.
fn bin_lines(ui: &mut egui::Ui, snapshot: &ArenaSnapshot) {
    ui.label(format!("{} / {} props", snapshot.bodies, snapshot.target));
    ui.label(format!(
        "physics {:.2} ms/step · worst {:.2} ms",
        snapshot.mean_step_milliseconds, snapshot.worst_step_milliseconds
    ));
    if snapshot.escaped > 0 {
        ui.label(RichText::new(format!("{} escaped the bin", snapshot.escaped)).color(WARNING));
    }
}

/// The sidebar's text, rule, and button look.
fn style(ui: &mut egui::Ui) {
    let visuals = ui.visuals_mut();
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, RULE);
    for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        widget.weak_bg_fill = BUTTON;
        widget.bg_fill = BUTTON;
        widget.bg_stroke = Stroke::new(1.0, BUTTON_LINE);
        widget.corner_radius = 4.0.into();
    }
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, TITLE);
    ui.spacing_mut().item_spacing.y = 6.0;
}
