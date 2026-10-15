// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Development overlay: frame rate, frame time, backend, viewport, renderer statistics, and game-published values.
//!
//! The Rust counterpart of the TypeScript `DevTools`: disabled in production, toggled with the backquote key, and fed
//! only by values that already exist (no GPU queries).

use indexmap::IndexMap;
use joy_render::RendererDiagnostics;

use crate::game::Viewport;
use crate::theme;

/// Number of frame intervals averaged for the frame-rate readout.
const SAMPLE_COUNT: usize = 60;

/// Development diagnostics overlay owned by the shell.
pub struct DevTools {
    // Configuration.
    title: String,
    enabled: bool,

    // Presentation state.
    expanded: bool,

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
            expanded: false,
            intervals: Vec::with_capacity(SAMPLE_COUNT),
            cursor: 0,
            values: IndexMap::new(),
        }
    }

    /// Whether the overlay is enabled (development builds).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Show or hide the detail panel.
    pub fn toggle(&mut self) {
        if self.enabled {
            self.expanded = !self.expanded;
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

    /// Draw the overlay. The shell calls this after the game's UI.
    pub(crate) fn ui(&mut self, context: &egui::Context, diagnostics: &RendererDiagnostics, viewport: Viewport) {
        if !self.enabled {
            return;
        }
        let milliseconds = self.frame_milliseconds();
        let fps = if milliseconds > 0.0 { 1000.0 / milliseconds } else { 0.0 };

        egui::Area::new(egui::Id::new("joy-dev-tools"))
            // Bottom center stays clear of the corners games use for titles, telemetry, and touch controls.
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0))
            .show(context, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    let summary =
                        format!("{} · {fps:.0} FPS · {milliseconds:.1} ms · {}", self.title, diagnostics.backend);
                    if ui
                        .add(egui::Label::new(egui::RichText::new(summary).monospace()).sense(egui::Sense::click()))
                        .clicked()
                    {
                        self.expanded = !self.expanded;
                    }
                    if !self.expanded {
                        return;
                    }
                    ui.separator();
                    egui::Grid::new("joy-dev-tools-grid").num_columns(2).show(ui, |ui| {
                        let mut row = |label: &str, value: String| {
                            ui.label(label);
                            ui.monospace(value);
                            ui.end_row();
                        };
                        row("adapter", diagnostics.adapter.clone());
                        row("hdr", diagnostics.hdr.to_string());
                        row(
                            "viewport",
                            format!(
                                "{:.0}×{:.0} @{:.2}",
                                viewport.logical_size.x, viewport.logical_size.y, viewport.scale_factor
                            ),
                        );
                        row("instances", format!("{} visible · {} culled", diagnostics.visible, diagnostics.culled));
                        row("draws", format!("{} calls · {} triangles", diagnostics.draw_calls, diagnostics.triangles));
                        row(
                            "shadows",
                            if diagnostics.shadows_active {
                                format!("{} casters", diagnostics.shadow_casters)
                            } else {
                                "off".to_owned()
                            },
                        );
                        for (key, value) in &self.values {
                            row(key, value.clone());
                        }
                    });
                });
            });
    }
}
