// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One tab's source and disk baseline. Editors borrow the current text; the workspace owns
//! persistence and conflicts.

use crate::server::DiskDocument;

/// Owns one open document's latest captured text, the text last read or written (its baseline), the disk revision, and
/// any unresolved disk conflict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDocument {
    /// Project id the document belongs to.
    pub project: String,
    /// Project-relative path.
    pub path: String,
    /// Latest text captured from the editor.
    pub text: String,
    /// Text that matches the disk revision.
    pub baseline: String,
    /// Disk revision of the baseline.
    pub revision: String,
    /// A newer disk version found while the document had unsaved edits.
    pub conflict: Option<DiskDocument>,
    /// Whether a save is in progress.
    pub saving: bool,
}

impl WorkspaceDocument {
    /// A clean document read from disk.
    pub fn new(project: &str, path: &str, disk: DiskDocument) -> Self {
        Self {
            project: project.to_owned(),
            path: path.to_owned(),
            text: disk.text.clone(),
            baseline: disk.text,
            revision: disk.revision,
            conflict: None,
            saving: false,
        }
    }

    /// Whether the captured text differs from the disk baseline.
    pub fn dirty(&self) -> bool {
        self.text != self.baseline
    }

    /// Replace the local source without advancing its saved baseline.
    pub fn edit(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    /// Capture source and revision together so edits made during a save stay dirty.
    pub fn snapshot(&self) -> DiskDocument {
        DiskDocument {
            text: self.text.clone(),
            revision: self.revision.clone(),
        }
    }

    /// Advance the baseline to the submitted snapshot, preserving any newer local text.
    pub fn saved(&mut self, snapshot: &DiskDocument, disk: &DiskDocument) {
        self.baseline = snapshot.text.clone();
        self.revision = disk.revision.clone();
        self.conflict = None;
    }

    /// Accept a clean external change, or keep a conflict for the author to resolve. Returns whether the editor should
    /// reload the new text.
    pub fn external(&mut self, disk: &DiskDocument) -> bool {
        if disk.revision == self.revision || self.saving {
            return false;
        }
        if self.dirty() {
            self.conflict = Some(disk.clone());
            return false;
        }
        self.text = disk.text.clone();
        self.baseline = disk.text.clone();
        self.revision = disk.revision.clone();
        self.conflict = None;
        true
    }

    /// Keep the local edit buffer and acknowledge the competing revision; saving then replaces the reviewed version.
    pub fn keep_local(&mut self) {
        if let Some(conflict) = self.conflict.take() {
            self.baseline = conflict.text;
            self.revision = conflict.revision;
        }
    }

    /// Discard local edits in favor of the retained conflicting disk version.
    pub fn reload_disk(&mut self) {
        if let Some(conflict) = self.conflict.take() {
            self.text = conflict.text.clone();
            self.baseline = conflict.text;
            self.revision = conflict.revision;
        }
    }
}
