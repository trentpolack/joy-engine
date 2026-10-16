// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`ParticleLabTool`]: PARTICLE LAB as a [`Game`] (standalone window or page) and an [`AuthoringTool`] (a Joy Editor
//! tab). It wires the session, the interface, and the engine renderer together: the simulation phase advances the
//! session, the render phase draws the accepted effect into the preview region the last interface pass left open, and
//! the UI phase draws the panels.
//!
//! Standalone, the tool restores and saves its working draft through `runtime::Storage` and owns Open and Save. Hosted,
//! it writes no recovery drafts, hides its header and footer, and the editor owns files through
//! [`AuthoringTool::open_document`] and [`AuthoringTool::save_document`].

use joy_engine::development::{AuthoringTool, ToolDocument, ToolHosting};
use joy_engine::egui;
use joy_engine::glam::Vec2;
use joy_engine::runtime::{Frame, Game, GameResult, RenderTarget, StartContext, Storage};

use crate::constants::STORAGE_APPLICATION;
use crate::document::write_asset;
use crate::interface::{Interface, apply_look};
use crate::lab::ParticleLab;
use crate::viewport::{PreviewRegion, post_processing};

/// The tool's display name.
pub const TOOL_NAME: &str = "PARTICLE LAB";

/// Document extensions the tool opens.
pub const DOCUMENT_EXTENSIONS: [&str; 1] = ["joyfx"];

/// PARTICLE LAB: session, interface, and renderer wiring.
pub struct ParticleLabTool {
    lab: ParticleLab,
    interface: Interface,
    hosting: ToolHosting,
    render_error: Option<String>,
}

impl Default for ParticleLabTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ParticleLabTool {
    /// A standalone tool with recovery and layout preferences in the per-user PARTICLE LAB storage.
    pub fn new() -> Self {
        Self::with_storage(Some(Storage::new(STORAGE_APPLICATION)))
    }

    /// A tool with explicit storage (`None` for a hosted tab or tests: no recovery, default layout).
    pub fn with_storage(storage: Option<Storage>) -> Self {
        Self {
            lab: ParticleLab::new(storage.clone()),
            interface: Interface::new(storage, false),
            hosting: ToolHosting::Standalone,
            render_error: None,
        }
    }

    /// The session.
    pub fn lab(&self) -> &ParticleLab {
        &self.lab
    }

    /// The session, for hosts and tests.
    pub fn lab_mut(&mut self) -> &mut ParticleLab {
        &mut self.lab
    }

    /// The interface state, for hosts and tests.
    pub fn interface_mut(&mut self) -> &mut Interface {
        &mut self.interface
    }

    /// The last render failure, shown over the preview.
    pub fn render_error(&self) -> Option<&str> {
        self.render_error.as_deref()
    }
}

impl Game for ParticleLabTool {
    fn start(&mut self, context: &mut StartContext<'_>) -> GameResult {
        context.renderer.set_post_processing(post_processing());
        let label = context.capabilities.backend.label().to_uppercase();
        self.interface.set_backend_label(if label == "WEBGL" { "WEBGL 2".to_owned() } else { label });
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> GameResult {
        self.lab.frame(f64::from(frame.time.elapsed_seconds));
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> GameResult {
        let size = frame.viewport.logical_size;
        let region = self
            .interface
            .region()
            .filter(|region| region.size.x > 1.0 && region.size.y > 1.0)
            .unwrap_or_else(|| PreviewRegion::full(size));
        let region = PreviewRegion { viewport_size: size, ..region };
        self.lab.frame_pending_effect(region.size.max(Vec2::ONE));
        // The viewport borrows the session's effect read-only while drawing.
        let (viewport, effect) = self.lab.viewport_and_effect();
        let result = viewport.render(target, effect, region);
        // A preview failure is shown over the viewport instead of stopping the tool.
        self.render_error = result.err();
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        if self.hosting == ToolHosting::Standalone {
            apply_look(context);
        }
        let area = match self.hosting {
            ToolHosting::Standalone => context.content_rect(),
            ToolHosting::Hosted { area } => area,
        };
        self.interface.show(context, &mut self.lab, area);
        if let (Some(message), Some(region)) = (&self.render_error, self.interface.region()) {
            let rect = egui::Rect::from_min_size(
                egui::pos2(region.min.x, region.min.y),
                egui::vec2(region.size.x, region.size.y),
            );
            egui::Area::new(egui::Id::new("particle-lab-gpu-error")).fixed_pos(rect.min + egui::vec2(12.0, 12.0)).show(
                context,
                |ui| {
                    ui.colored_label(
                        joy_engine::runtime::theme::tokens::DANGER,
                        format!("GPU preview unavailable: {message}"),
                    );
                },
            );
        }
    }
}

impl AuthoringTool for ParticleLabTool {
    fn tool_name(&self) -> &'static str {
        TOOL_NAME
    }

    fn document_extensions(&self) -> &'static [&'static str] {
        &DOCUMENT_EXTENSIONS
    }

    fn open_document(&mut self, document: ToolDocument) -> Result<(), String> {
        let asset = crate::document::read_asset(&document.contents)?;
        self.lab.load_document(asset);
        Ok(())
    }

    fn save_document(&mut self, _name: &str) -> Result<String, String> {
        self.lab.drafts.require_valid()?;
        Ok(write_asset(self.lab.asset()))
    }

    fn has_unsaved_changes(&self) -> bool {
        self.lab.has_unsaved_changes()
    }

    fn mark_saved(&mut self) {
        self.lab.mark_saved();
    }

    fn set_hosting(&mut self, hosting: ToolHosting) {
        let hosted = matches!(hosting, ToolHosting::Hosted { .. });
        if hosted != matches!(self.hosting, ToolHosting::Hosted { .. }) {
            // Hosted tabs leave files and recovery to the editor.
            self.lab.set_storage(if hosted { None } else { Some(Storage::new(STORAGE_APPLICATION)) });
            self.interface.set_hosted(hosted);
        }
        self.hosting = hosting;
    }

    fn activated(&mut self, context: &mut StartContext<'_>) {
        context.renderer.set_post_processing(post_processing());
    }
}
