// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Hosting authoring tools as workspace tabs (TypeScript `tool-bridge.ts`).
//!
//! The TypeScript workspace loaded each tool into a same-origin frame and exchanged load, capture, and save messages
//! with it. The Rust editor embeds tools in process through `joy_engine::development::AuthoringTool`: a [`ToolTab`]
//! owns one tool instance, starts it with the shared renderer the first time a frame can provide one, then hands it the
//! document; captures call `save_document`, saves call `mark_saved`, and the workspace drives phases only for the
//! active tab, so inactive previews suspend. [`ToolRegistry`] maps extensions to tool factories (FORM LAB for `.form`
//! and `.formlab`, PARTICLE LAB for `.joyfx`); tests register their own tools.

use joy_engine::development::{AuthoringTool, ToolDocument, ToolHosting};
use joy_engine::egui;
use joy_engine::runtime::StartContext;

/// Creates a fresh tool instance.
pub type ToolFactory = Box<dyn Fn() -> Box<dyn AuthoringTool>>;

/// One registered tool.
struct ToolRegistration {
    name: String,
    extensions: Vec<String>,
    create: ToolFactory,
}

/// Tools available for hosting, by document extension.
#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<ToolRegistration>,
}

impl ToolRegistry {
    /// No tools.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool factory; its name and extensions are read from one probe instance. Later registrations win for
    /// shared extensions.
    pub fn register(&mut self, create: impl Fn() -> Box<dyn AuthoringTool> + 'static) {
        let probe = create();
        let name = probe.tool_name().to_owned();
        let extensions = probe.document_extensions().iter().map(|extension| extension.to_lowercase()).collect();
        self.tools.insert(0, ToolRegistration { name, extensions, create: Box::new(create) });
    }

    /// A new tool instance for documents with `extension` (without the dot).
    pub fn create(&self, extension: &str) -> Option<Box<dyn AuthoringTool>> {
        self.find(extension).map(|registration| (registration.create)())
    }

    /// The display name of the tool for `extension`.
    pub fn tool_name(&self, extension: &str) -> Option<&str> {
        self.find(extension).map(|registration| registration.name.as_str())
    }

    /// Registered tool names, in registration order.
    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().rev().map(|registration| registration.name.as_str()).collect()
    }

    fn find(&self, extension: &str) -> Option<&ToolRegistration> {
        let extension = extension.to_lowercase();
        self.tools.iter().find(|registration| registration.extensions.contains(&extension))
    }
}

/// One hosted tool and the document it will open once started.
pub struct ToolTab {
    // The tool and its hosting state.
    tool: Box<dyn AuthoringTool>,
    name: String,
    started: bool,
    activation_pending: bool,
    area: Option<egui::Rect>,

    // The document to open after start; afterwards documents go straight to the tool.
    pending: Option<ToolDocument>,
}

impl ToolTab {
    /// Host `tool` for `document`; the tool starts with the next frame that has a renderer.
    pub fn new(tool: Box<dyn AuthoringTool>, document: ToolDocument) -> Self {
        let name = tool.tool_name().to_owned();
        Self { tool, name, started: false, activation_pending: true, area: None, pending: Some(document) }
    }

    /// The tool's display name.
    pub fn tool_name(&self) -> &str {
        &self.name
    }

    /// Whether `Game::start` ran.
    pub fn started(&self) -> bool {
        self.started
    }

    /// Start the tool and open its document. A rejected document returns the tool's message so the workspace can show
    /// a repair view; the tool stays started.
    pub fn prepare(&mut self, context: &mut StartContext<'_>) -> Result<(), String> {
        if !self.started {
            self.started = true;
            self.tool.set_hosting(ToolHosting::Hosted { area: self.area.unwrap_or(egui::Rect::ZERO) });
            self.tool.start(context).map_err(|error| error.to_string())?;
            self.activation_pending = true;
        }
        match self.pending.take() {
            Some(document) => self.tool.open_document(document),
            None => Ok(()),
        }
    }

    /// Ask for `AuthoringTool::activated` before the next frame (the tab became active, or another tool changed the
    /// shared renderer).
    pub fn request_activation(&mut self) {
        self.activation_pending = true;
    }

    /// Re-apply the tool's renderer settings when activation is pending.
    pub fn activate(&mut self, context: &mut StartContext<'_>) {
        if self.started && std::mem::take(&mut self.activation_pending) {
            self.tool.activated(context);
        }
    }

    /// Place the tool in the tab area (logical points); unchanged areas are not re-sent.
    pub fn set_area(&mut self, area: egui::Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.tool.set_hosting(ToolHosting::Hosted { area });
        }
    }

    /// The tab area last given to the tool.
    pub fn area(&self) -> Option<egui::Rect> {
        self.area
    }

    /// Replace the tool's document (an external edit or a repaired source).
    pub fn open(&mut self, document: ToolDocument) -> Result<(), String> {
        if self.started {
            self.tool.open_document(document)
        } else {
            self.pending = Some(document);
            Ok(())
        }
    }

    /// The text to save as `name`; an unstarted tool still holds the disk text.
    pub fn capture(&mut self, name: &str) -> Result<String, String> {
        match &self.pending {
            Some(document) if !self.started => Ok(document.contents.clone()),
            _ => self.tool.save_document(name),
        }
    }

    /// Whether the tool reports edits since its document was opened or saved.
    pub fn has_unsaved_changes(&self) -> bool {
        self.started && self.tool.has_unsaved_changes()
    }

    /// Record that the workspace wrote the document.
    pub fn mark_saved(&mut self) {
        if self.started {
            self.tool.mark_saved();
        }
    }

    /// The hosted tool, for its frame phases.
    pub fn tool_mut(&mut self) -> &mut dyn AuthoringTool {
        self.tool.as_mut()
    }
}
