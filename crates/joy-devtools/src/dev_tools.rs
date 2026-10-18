// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Development overlay: frame rate, frame time, backend, viewport, renderer statistics, and game-published values.
//!
//! Disabled in production, with a STATS launcher in the development dock,
//! toggled with the backquote key, and fed only by values that already exist (no GPU queries).

use egui::RichText;
use indexmap::IndexMap;

use crate::{
    dock,
    dock::DockTool,
};
use joy_renderer::RendererDiagnostics;
use joy_ui::Viewport;

/// Viewports at most this wide show only the launcher's label, like the TypeScript widget.
const COMPACT_WIDTH: f32 = 448.0;

/// Number of frame intervals averaged for the frame-rate readout.
const SAMPLE_COUNT: usize = 60;

/// Development diagnostics overlay owned by the shell.
pub struct DevTools {
    // Configuration.
    title: String,
    enabled: bool,

    // A backquote toggle waiting for the next UI pass.
    toggle_requested: bool,

    // Rolling frame samples in seconds.
    intervals: Vec<f32>,
    cursor: usize,

    // Game-published key/value diagnostics, in insertion order.
    values: IndexMap<String, String>,
}

impl DevTools {
    /// Create an overlay; when `enabled` is false it never draws and ignores the toggle.
    pub fn new(title: &str, enabled: bool) -> Self {
        Self {
            title: title.to_owned(),
            enabled,
            toggle_requested: false,
            intervals: Vec::with_capacity(SAMPLE_COUNT),
            cursor: 0,
            values: IndexMap::new(),
        }
    }

    /// Whether the overlay is enabled (development builds).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Show or hide the detail panel on the next UI pass.
    pub fn toggle(&mut self) {
        if self.enabled {
            self.toggle_requested = !self.toggle_requested;
        }
    }

    /// Write a development message to the log; disabled overlays stay silent.
    pub fn log(&self, message: &str) {
        if self.enabled {
            log::info!("[Joy Dev] {message}");
        }
    }

    /// Publish or replace a diagnostic value shown in the detail panel.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.values.insert(key.to_owned(), value.into());
    }

    /// Record one frame interval in seconds.
    pub fn frame(&mut self, elapsed_seconds: f32) {
        if elapsed_seconds <= 0.0 {
            return;
        }
        if self.intervals.len() < SAMPLE_COUNT {
            self.intervals.push(elapsed_seconds);
        } else {
            self.intervals[self.cursor] = elapsed_seconds;
        }
        self.cursor = (self.cursor + 1) % SAMPLE_COUNT;
    }

    /// Average frame time in milliseconds over the sample window.
    pub fn frame_milliseconds(&self) -> f32 {
        if self.intervals.is_empty() {
            return 0.0;
        }
        (self.intervals.iter().sum::<f32>() / self.intervals.len() as f32) * 1000.0
    }

    /// Draw the launcher and, when open, the detail panel. The shell calls this after the game's UI.
    #[doc(hidden)]
    pub fn ui(&mut self, context: &egui::Context, diagnostics: &RendererDiagnostics, viewport: Viewport) {
        if !self.enabled {
            return;
        }
        if std::mem::take(&mut self.toggle_requested) {
            let open = dock::open_tool(context) == Some(DockTool::Stats);
            dock::set_open(context, DockTool::Stats, !open);
        }
        let milliseconds = self.frame_milliseconds();
        let fps = if milliseconds > 0.0 { 1000.0 / milliseconds } else { 0.0 };
        let label = if context.content_rect().width() <= COMPACT_WIDTH {
            "STATS".to_owned()
        } else if milliseconds > 0.0 {
            format!("STATS | {fps:.0} FPS")
        } else {
            "STATS | -- FPS".to_owned()
        };
        dock::launcher(context, DockTool::Stats, RichText::new(label));
        dock::panel(context, DockTool::Stats, &self.title, |ui| {
            ui.label(RichText::new(format!("{milliseconds:.1} MS")).color(dock::ACCENT))
                .on_hover_text("Frame interval, including waits; not CPU or GPU execution time");
            dock::note(ui, "Press ` to toggle runtime details.");
            let rows = |ui: &mut egui::Ui, id: &str, rows: Vec<(String, String)>| {
                // Long values (adapter names) wrap inside the dock's panel width instead of widening it.
                let value_width = (ui.available_width() * 0.6).max(120.0);
                egui::Grid::new(id).num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                    for (label, value) in rows {
                        ui.label(RichText::new(label).color(dock::MUTED));
                        ui.scope(|ui| {
                            ui.set_max_width(value_width);
                            ui.add(egui::Label::new(value).wrap());
                        });
                        ui.end_row();
                    }
                });
            };
            egui::CollapsingHeader::new("Runtime").default_open(true).show(ui, |ui| {
                let mut values = vec![
                    ("mode".to_owned(), "development".to_owned()),
                    ("backend".to_owned(), diagnostics.backend.to_string()),
                    ("adapter".to_owned(), diagnostics.adapter.clone()),
                    (
                        "viewport".to_owned(),
                        format!(
                            "{:.0} × {:.0} @{:.2}",
                            viewport.logical_size.x, viewport.logical_size.y, viewport.scale_factor
                        ),
                    ),
                ];
                values.extend(self.values.iter().map(|(key, value)| (key.clone(), value.clone())));
                rows(ui, "joy-dev-runtime", values);
            });
            egui::CollapsingHeader::new("Timing").default_open(true).show(ui, |ui| {
                rows(
                    ui,
                    "joy-dev-timing",
                    vec![
                        ("FPS".to_owned(), format!("{fps:.0}")),
                        ("frame interval".to_owned(), format!("{milliseconds:.1} ms")),
                        ("CPU time".to_owned(), "Not collected".to_owned()),
                        ("GPU time".to_owned(), "Not collected".to_owned()),
                    ],
                );
            });
            egui::CollapsingHeader::new("GPU Resources").default_open(false).show(ui, |ui| {
                let shadows = if diagnostics.shadows_active {
                    format!("{} casters", diagnostics.shadow_casters)
                } else {
                    "off".to_owned()
                };
                rows(
                    ui,
                    "joy-dev-gpu",
                    vec![
                        ("HDR scene".to_owned(), diagnostics.hdr.to_string()),
                        (
                            "this scene visible / culled".to_owned(),
                            format!("{} / {}", diagnostics.visible, diagnostics.culled),
                        ),
                        ("draw calls".to_owned(), diagnostics.draw_calls.to_string()),
                        ("triangles".to_owned(), diagnostics.triangles.to_string()),
                        ("shadows".to_owned(), shadows),
                    ],
                );
            });
        });
    }
}
