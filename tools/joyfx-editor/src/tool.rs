// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`JoyfxEditorTool`]: JOYFX EDITOR as a [`App`] (standalone window or page) and an [`AuthoringTool`] (a Joy Editor
//! tab). It wires the session, the interface, and the engine renderer together: the simulation phase advances the
//! session, the render phase draws the accepted effect into the preview region the last interface pass left open, and
//! the UI phase draws the panels.
//!
//! Standalone, the tool restores and saves its working draft through `runtime::Storage` and owns Open and Save. Hosted,
//! it writes no recovery drafts, hides its header and footer, and the editor owns files through
//! [`AuthoringTool::open_document`] and [`AuthoringTool::save_document`].

use joy_engine::development::{
    AuthoringTool,
    ToolDocument,
    ToolHosting,
};
use joy_engine::egui;
use joy_engine::math::Vec2;
use joy_engine::runtime::{
    App,
    AppResult,
    Frame,
    RenderTarget,
    StartContext,
    Storage,
};

use crate::constants::STORAGE_APPLICATION;
use crate::document::write_asset;
use crate::interface::{
    Interface,
    apply_look,
};
use crate::session::JoyfxEditor;
use crate::viewport::{
    PreviewRegion,
    post_process_settings,
};

/// The tool's display name.
pub const TOOL_NAME: &str = "JOYFX EDITOR";

/// Document extensions the tool opens.
pub const DOCUMENT_EXTENSIONS: [&str; 1] = ["joyfx"];

/// JOYFX EDITOR: session, interface, and renderer wiring.
pub struct JoyfxEditorTool {
    session: JoyfxEditor,
    interface: Interface,
    hosting: ToolHosting,
    render_error: Option<String>,
}

impl Default for JoyfxEditorTool {
    fn default() -> Self {
        Self::new()
    }
}

impl JoyfxEditorTool {
    /// A standalone tool with recovery and layout preferences in the per-user JOYFX EDITOR storage.
    pub fn new() -> Self {
        Self::with_storage(Some(Storage::new(STORAGE_APPLICATION)))
    }

    /// A tool with explicit storage (`None` for a hosted tab or tests: no recovery, default layout).
    pub fn with_storage(storage: Option<Storage>) -> Self {
        Self {
            session: JoyfxEditor::new(storage.clone()),
            interface: Interface::new(storage, false),
            hosting: ToolHosting::Standalone,
            render_error: None,
        }
    }

    /// The session.
    pub fn session(&self) -> &JoyfxEditor {
        &self.session
    }

    /// The session, for hosts and tests.
    pub fn session_mut(&mut self) -> &mut JoyfxEditor {
        &mut self.session
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

impl App for JoyfxEditorTool {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        context.renderer.set_post_process(post_process_settings());
        let label = context.capabilities.backend.label().to_uppercase();
        self.interface.set_backend_label(if label == "WEBGL" { "WEBGL 2".to_owned() } else { label });
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        self.session.frame(f64::from(frame.time.elapsed_seconds));
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        let size = frame.viewport.logical_size;
        let region = self
            .interface
            .region()
            .filter(|region| region.size.x > 1.0 && region.size.y > 1.0)
            .unwrap_or_else(|| PreviewRegion::full(size));
        let region = PreviewRegion {
            viewport_size: size,
            ..region
        };
        self.session.frame_pending_effect(region.size.max(Vec2::ONE));
        // The viewport borrows the session's effect read-only while drawing.
        let (viewport, effect) = self.session.viewport_and_effect();
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
            ToolHosting::Hosted {
                area,
            } => area,
        };
        self.interface.show(context, &mut self.session, area);
        if let (Some(message), Some(region)) = (&self.render_error, self.interface.region()) {
            let rect = egui::Rect::from_min_size(
                egui::pos2(region.min.x, region.min.y),
                egui::vec2(region.size.x, region.size.y),
            );
            egui::Area::new(egui::Id::new("joyfx-editor-gpu-error")).fixed_pos(rect.min + egui::vec2(12.0, 12.0)).show(
                context,
                |ui| {
                    ui.colored_label(
                        joy_engine::runtime::theme::colors::DANGER,
                        format!("GPU preview unavailable: {message}"),
                    );
                },
            );
        }
    }
}

impl AuthoringTool for JoyfxEditorTool {
    fn tool_name(&self) -> &'static str {
        TOOL_NAME
    }

    fn document_extensions(&self) -> &'static [&'static str] {
        &DOCUMENT_EXTENSIONS
    }

    fn open_document(&mut self, document: ToolDocument) -> Result<(), String> {
        let asset = crate::document::read_asset(&document.contents)?;
        self.session.load_document(asset);
        Ok(())
    }

    fn save_document(&mut self, _name: &str) -> Result<String, String> {
        self.session.drafts.require_valid()?;
        Ok(write_asset(self.session.asset()))
    }

    fn has_unsaved_changes(&self) -> bool {
        self.session.has_unsaved_changes()
    }

    fn mark_saved(&mut self) {
        self.session.mark_saved();
    }

    fn set_hosting(&mut self, hosting: ToolHosting) {
        let hosted = matches!(hosting, ToolHosting::Hosted { .. });
        if hosted != matches!(self.hosting, ToolHosting::Hosted { .. }) {
            // Hosted tabs leave files and recovery to the editor.
            self.session.set_storage(if hosted { None } else { Some(Storage::new(STORAGE_APPLICATION)) });
            self.interface.set_hosted(hosted);
        }
        self.hosting = hosting;
    }

    fn activated(&mut self, context: &mut StartContext<'_>) {
        context.renderer.set_post_process(post_process_settings());
    }
}
