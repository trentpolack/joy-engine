// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Development overlay: frame rate, frame time, backend, viewport, renderer statistics and profiling, game-published
//! values, and the console.
//!
//! Disabled in production. The STATS launcher in the development dock opens the statistics panel; the backquote key
//! opens the [`Console`], whose `stat` commands show frame and per-phase timing overlays and whose `r.` variables the
//! shell applies to the renderer each frame ([`DevTools::apply_to_renderer`]).

use egui::RichText;
use indexmap::IndexMap;

use crate::console::{
    Console,
    LineKind,
};
use crate::console_variables::ConsoleVariables;
use crate::render_variables::{
    self,
    RenderVariables,
};
use crate::{
    dock,
    dock::DockTool,
};
use joy_renderer::{
    DebugView,
    FrameProfile,
    Renderer,
    RendererDiagnostics,
};
use joy_ui::Viewport;

/// Viewports at most this wide show only the launcher's label.
const COMPACT_WIDTH: f32 = 448.0;

/// Number of frame intervals averaged for the frame-rate readout.
const SAMPLE_COUNT: usize = 60;

/// Distance of the stat overlays from the screen's top-right corner, in logical pixels.
const STAT_MARGIN: f32 = 12.0;

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

    // App-published key/value diagnostics, in insertion order.
    values: IndexMap<String, String>,

    // The console, its variables, and the rendering settings they park.
    console: Console,
    render_variables: RenderVariables,
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
            console: Console::new(),
            render_variables: RenderVariables::default(),
        }
    }

    /// Whether the overlay is enabled (development builds).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Show or hide the statistics panel on the next UI pass.
    pub fn toggle(&mut self) {
        if self.enabled {
            self.toggle_requested = !self.toggle_requested;
        }
    }

    /// Show or hide the console (the shell's backquote key).
    pub fn toggle_console(&mut self) {
        if self.enabled {
            self.console.toggle();
        }
    }

    /// The console, for running commands and printing lines.
    pub fn console(&mut self) -> &mut Console {
        &mut self.console
    }

    /// The console variables, for registering a game's own variables and reading them each frame. They exist in
    /// production builds too, at their defaults, though only development builds open the console.
    pub fn console_variables(&mut self) -> &mut ConsoleVariables {
        self.console.variables_mut()
    }

    /// Apply changed `r.` console variables to `renderer`, then show its actual state in them. The shell calls this
    /// every frame before the game renders; refused changes print to the console.
    pub fn apply_to_renderer(&mut self, renderer: &mut Renderer) {
        if !self.enabled {
            return;
        }
        let refusals = self.render_variables.apply(self.console.variables_mut(), renderer);
        for refusal in refusals {
            self.console.print(LineKind::Error, refusal);
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

    /// Draw the launcher, the statistics panel when open, the stat overlays, and the console. The shell calls this
    /// after the game's UI with the renderer's latest diagnostics and profile.
    #[doc(hidden)]
    pub fn ui(
        &mut self,
        context: &egui::Context,
        diagnostics: &RendererDiagnostics,
        profile: Option<&FrameProfile>,
        viewport: Viewport,
    ) {
        if !self.enabled {
            return;
        }
        self.stat_overlays(context, profile);
        self.console.ui(context);
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
        let title = self.title.clone();
        dock::panel(context, DockTool::Stats, &title, |ui| {
            ui.label(RichText::new(format!("{milliseconds:.1} MS")).color(dock::ACCENT))
                .on_hover_text("Frame interval, including waits; not CPU or GPU execution time");
            dock::note(ui, "Press ` for the console; stat unit and stat gpu show timing overlays.");
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
                    ("display output".to_owned(), diagnostics.display.to_string()),
                ];
                // Scene frames report the size they rendered at; below the drawing buffer when the render scale is.
                let (render_width, render_height) = diagnostics.render_size;
                if render_width > 0 && render_height > 0 {
                    values.push(("scene render size".to_owned(), format!("{render_width} × {render_height}")));
                }
                values.extend(self.values.iter().map(|(key, value)| (key.clone(), value.clone())));
                rows(ui, "joy-dev-runtime", values);
            });
            egui::CollapsingHeader::new("Timing").default_open(true).show(ui, |ui| {
                let (cpu, gpu) = match profile {
                    Some(profile) => (format!("{:.2} ms recording", profile.cpu_milliseconds), gpu_label(profile)),
                    None => ("Profiling off".to_owned(), "Profiling off".to_owned()),
                };
                rows(
                    ui,
                    "joy-dev-timing",
                    vec![
                        ("FPS".to_owned(), format!("{fps:.0}")),
                        ("frame interval".to_owned(), format!("{milliseconds:.1} ms")),
                        ("CPU time".to_owned(), cpu),
                        ("GPU time".to_owned(), gpu),
                    ],
                );
                if let Some(profile) = profile {
                    let phases = profile
                        .phases
                        .iter()
                        .map(|timing| {
                            let gpu =
                                timing.gpu_milliseconds.map_or_else(|| "--".to_owned(), |gpu| format!("{gpu:.2}"));
                            (
                                timing.phase.label().to_owned(),
                                format!("{gpu} GPU · {:.2} CPU ms", timing.cpu_milliseconds),
                            )
                        })
                        .collect();
                    rows(ui, "joy-dev-phases", phases);
                }
            });
            egui::CollapsingHeader::new("Rendering").default_open(false).show(ui, |ui| {
                self.rendering_controls(ui);
            });
            egui::CollapsingHeader::new("GPU Resources").default_open(false).show(ui, |ui| {
                let shadows = if diagnostics.shadows_active || diagnostics.shadow_point_lights > 0 {
                    format!(
                        "{} casters, {} point lights, {} views",
                        diagnostics.shadow_casters, diagnostics.shadow_point_lights, diagnostics.shadow_views
                    )
                } else {
                    "off".to_owned()
                };
                let lighting = if diagnostics.deferred {
                    format!(
                        "deferred, {} lighting passes, {} forward batches",
                        diagnostics.lighting_passes, diagnostics.forward_opaque_batches
                    )
                } else {
                    "forward".to_owned()
                };
                let lights = format!(
                    "{} drawn, {} omitted, {} cluster references ({} dropped)",
                    diagnostics.point_lights,
                    diagnostics.point_lights_omitted,
                    diagnostics.cluster_light_references,
                    diagnostics.cluster_references_dropped
                );
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
                        ("lighting".to_owned(), lighting),
                        ("point lights".to_owned(), lights),
                        ("shadows".to_owned(), shadows),
                    ],
                );
            });
        });
    }

    /// The debug view, profiling, and deferred lighting switches, written through their console variables so the console
    /// and panel agree.
    fn rendering_controls(&mut self, ui: &mut egui::Ui) {
        let variables = self.console.variables_mut();
        let current = variables.text(render_variables::DEBUG_VIEW).and_then(DebugView::from_name).unwrap_or_default();
        let mut selected = current;
        egui::ComboBox::from_label("Debug view").selected_text(selected.label()).show_ui(ui, |ui| {
            for view in DebugView::ALL {
                ui.selectable_value(&mut selected, view, view.label());
            }
        });
        if selected != current {
            let _ = variables.set(render_variables::DEBUG_VIEW, selected.name());
        }
        let mut profiling = variables.bool(render_variables::PROFILE).unwrap_or(false);
        if ui.checkbox(&mut profiling, "Profile frame phases").changed() {
            let _ = variables.set(render_variables::PROFILE, if profiling { "1" } else { "0" });
        }
        let mut deferred = variables.bool(render_variables::DEFERRED).unwrap_or(true);
        if ui.checkbox(&mut deferred, "Deferred lighting (where supported)").changed() {
            let _ = variables.set(render_variables::DEFERRED, if deferred { "1" } else { "0" });
        }
    }

    /// The `stat unit` and `stat gpu` overlays in the top-right corner.
    fn stat_overlays(&self, context: &egui::Context, profile: Option<&FrameProfile>) {
        let stats = self.console.stats();
        if !stats.unit && !stats.gpu {
            return;
        }
        let mut lines = Vec::new();
        if stats.unit {
            lines.push(format!("Frame {:>7.2} ms", self.frame_milliseconds()));
            match profile {
                Some(profile) => {
                    lines.push(format!("CPU   {:>7.2} ms", profile.cpu_milliseconds));
                    lines.push(format!("GPU   {:>10}", gpu_label(profile)));
                }
                None => lines.push("Waiting for profiling…".to_owned()),
            }
        }
        if stats.gpu {
            match profile {
                Some(profile) => lines.extend(profile.phases.iter().map(|timing| {
                    let gpu = timing.gpu_milliseconds.map_or_else(|| "     --".to_owned(), |gpu| format!("{gpu:>7.2}"));
                    format!("{:<18}{gpu} ms", timing.phase.label())
                })),
                None => lines.push("Waiting for profiling…".to_owned()),
            }
        }
        // Below the console while it is open, so neither hides the other.
        let screen = context.content_rect();
        let corner = screen.right_top() + egui::vec2(-STAT_MARGIN, STAT_MARGIN + self.console.covered_height(screen));
        egui::Area::new(egui::Id::new("joy-stat-overlays"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(corner)
            .interactable(false)
            .show(context, |ui| {
                egui::Frame::new().fill(dock::BACKGROUND).inner_margin(6.0).show(ui, |ui| {
                    for line in lines {
                        ui.label(RichText::new(line).monospace().color(dock::TEXT));
                    }
                });
            });
    }
}

/// A profile's GPU total, or why there is none.
fn gpu_label(profile: &FrameProfile) -> String {
    profile.gpu_milliseconds.map_or_else(|| "no timestamp queries".to_owned(), |gpu| format!("{gpu:.2} ms"))
}
