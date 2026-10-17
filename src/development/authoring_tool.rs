// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The contract between authoring tools (FORM LAB, PARTICLE LAB) and Joy Editor.
//!
//! A tool is a [`Game`]: run standalone it gets its own window or page through [`run`](crate::runtime::run) and owns
//! its file workflow (open, save, export). Hosted in Joy Editor it becomes a tab: the editor owns project files and
//! drives the active tab's frame phases, hands documents in with [`AuthoringTool::open_document`], asks for their
//! contents with [`AuthoringTool::save_document`], and keeps the tool's UI inside the tab area given by
//! [`ToolHosting::Hosted`].
//! Inactive tabs receive no phases, so their previews suspend; [`AuthoringTool::activated`] lets a tab re-apply its
//! renderer settings (post-processing, scene effects, image lighting) when it becomes active again, because every tab
//! shares the editor's one renderer.

use crate::runtime::{
    Game,
    StartContext,
};

/// A text document a tool opens or saves: a file name with its extension and the file's contents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolDocument {
    /// File name with extension, such as `tower.formlab`; the extension selects the format.
    pub name: String,
    /// The file's text.
    pub contents: String,
}

/// Where a tool runs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ToolHosting {
    /// Its own window or page, with its own file chrome.
    #[default]
    Standalone,
    /// A Joy Editor tab. The tool draws only inside `area` (logical points), hides its own open and save chrome (the
    /// editor saves through [`AuthoringTool::save_document`]), and frames its preview inside `area`; the editor draws
    /// its chrome over the rest of the window.
    Hosted {
        /// The tab's content area in logical points.
        area: egui::Rect,
    },
}

/// Implemented by authoring tools so the same tool runs standalone and as a Joy Editor tab.
pub trait AuthoringTool: Game {
    /// Display name, for example `FORM LAB`.
    fn tool_name(&self) -> &'static str;

    /// File extensions the tool opens, without dots, for example `["form", "formlab"]`.
    fn document_extensions(&self) -> &'static [&'static str];

    /// Replace the working document. A document the tool cannot accept returns an error and leaves the current
    /// document in place; tools that offer a source-repair view may instead accept it and show that view.
    fn open_document(&mut self, document: ToolDocument) -> Result<(), String>;

    /// The contents to write for `name`; the extension picks the format (for example raw script for `.form` and the
    /// full document for `.formlab`). Fails when the current document cannot be saved, such as an invalid draft.
    fn save_document(&mut self, name: &str) -> Result<String, String>;

    /// Whether the document changed since it was opened or last marked saved.
    fn has_unsaved_changes(&self) -> bool;

    /// Record that the host wrote the current document.
    fn mark_saved(&mut self);

    /// Switch between standalone and hosted presentation; hosts call this before each frame's phases when the tab
    /// area moves.
    fn set_hosting(&mut self, hosting: ToolHosting);

    /// The tab became active: re-apply renderer settings the tool owns. Called before the first frame after
    /// activation; [`Game::start`] has already run once.
    fn activated(&mut self, _context: &mut StartContext<'_>) {}
}
