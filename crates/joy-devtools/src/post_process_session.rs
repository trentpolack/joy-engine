// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One editing session over a post-process volume's saved settings: draft edits with undo and redo, a saved baseline,
//! and conflicts with external file changes.

use serde_json::Value;

use joy_core::JsonRecord;
use joy_core::format::json_number;
use joy_renderer::{
    PostProcessError,
    PostProcessOverrides,
    PostProcessSettings,
};

/// Undo steps retained per session.
const MAXIMUM_HISTORY_LENGTH: usize = 100;

/// A new value for one post-processing control.
#[derive(Clone, Debug, PartialEq)]
pub enum PostProcessValue {
    /// A numeric control's value.
    Number(f64),
    /// An enumerated control's spelling.
    Choice(String),
}

impl PostProcessValue {
    /// The JSON value stored in a component record; integral numbers stay integers.
    fn to_json(&self) -> Value {
        match self {
            Self::Number(number) => json_number(*number),
            Self::Choice(choice) => Value::String(choice.clone()),
        }
    }
}

/// External settings and their revision that arrived while the draft had unsaved edits.
#[derive(Clone, Debug, PartialEq)]
pub struct PostProcessConflict {
    /// The settings on disk.
    pub overrides: PostProcessOverrides,
    /// The disk revision.
    pub revision: String,
}

/// Draft edits, history, and the saved baseline for one volume's settings.
#[derive(Clone, Debug)]
pub struct PostProcessSession {
    saved: PostProcessOverrides,
    draft: PostProcessOverrides,
    revision: String,
    saving: bool,
    conflict: Option<PostProcessConflict>,
    undo_history: Vec<PostProcessOverrides>,
    redo_history: Vec<PostProcessOverrides>,
}

impl PostProcessSession {
    /// Start from a volume's saved settings at a revision (an opaque content identity, or empty when unknown).
    pub fn new(overrides: PostProcessOverrides, revision: impl Into<String>) -> Self {
        Self {
            saved: overrides.clone(),
            draft: overrides,
            revision: revision.into(),
            saving: false,
            conflict: None,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
        }
    }

    /// The edited settings.
    pub fn draft(&self) -> &PostProcessOverrides {
        &self.draft
    }

    /// The last saved or loaded settings.
    pub fn saved(&self) -> &PostProcessOverrides {
        &self.saved
    }

    /// The draft applied over the engine defaults: the values controls display.
    pub fn settings(&self) -> PostProcessSettings {
        PostProcessSettings::DEFAULT.with_overrides(&self.draft)
    }

    /// The revision the saved settings came from.
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Whether the draft differs from the saved settings.
    pub fn dirty(&self) -> bool {
        !self.draft.same_values(&self.saved)
    }

    /// Whether a save is in flight.
    pub fn saving(&self) -> bool {
        self.saving
    }

    /// An external change that needs [`keep_local`](Self::keep_local) or [`use_disk`](Self::use_disk).
    pub fn conflict(&self) -> Option<&PostProcessConflict> {
        self.conflict.as_ref()
    }

    /// Whether undo has a step.
    pub fn can_undo(&self) -> bool {
        !self.undo_history.is_empty()
    }

    /// Whether redo has a step.
    pub fn can_redo(&self) -> bool {
        !self.redo_history.is_empty()
    }

    /// Replace the draft with a settings record as one undoable step; invalid settings leave the session unchanged.
    pub fn replace(&mut self, record: &JsonRecord) -> Result<(), PostProcessError> {
        let overrides = PostProcessOverrides::from_record(record)?;
        self.replace_draft(overrides);
        Ok(())
    }

    /// Set one control's value.
    pub fn set_value(&mut self, key: &str, value: PostProcessValue) -> Result<(), PostProcessError> {
        let mut overrides = self.draft.clone();
        overrides.set(key, value.to_json())?;
        self.replace_draft(overrides);
        Ok(())
    }

    /// Remove one control's value so it inherits again.
    pub fn reset_value(&mut self, key: &str) {
        let mut overrides = self.draft.clone();
        if overrides.remove(key) {
            self.replace_draft(overrides);
        }
    }

    /// Step back one edit.
    pub fn undo(&mut self) {
        if let Some(previous) = self.undo_history.pop() {
            self.redo_history.push(std::mem::replace(&mut self.draft, previous));
        }
    }

    /// Step forward one undone edit.
    pub fn redo(&mut self) {
        if let Some(next) = self.redo_history.pop() {
            self.undo_history.push(std::mem::replace(&mut self.draft, next));
        }
    }

    /// Return the draft to the saved settings as one undoable step.
    pub fn revert(&mut self) {
        self.replace_draft(self.saved.clone());
    }

    /// Receive the settings on disk. A clean draft adopts them; unsaved edits that differ raise a conflict instead. The
    /// revision this session already holds clears any conflict (an echo of its own save, or a restored file).
    pub fn receive_external(&mut self, overrides: PostProcessOverrides, revision: &str) {
        if revision == self.revision {
            self.conflict = None;
            return;
        }
        if (self.dirty() || self.saving) && !overrides.same_values(&self.draft) {
            self.conflict = Some(PostProcessConflict {
                overrides,
                revision: revision.to_owned(),
            });
            return;
        }
        self.adopt_disk(overrides, revision.to_owned());
    }

    /// Keep the draft and treat the conflicting disk settings as the saved baseline.
    pub fn keep_local(&mut self) {
        if let Some(conflict) = self.conflict.take() {
            self.saved = conflict.overrides;
            self.revision = conflict.revision;
        }
    }

    /// Discard the draft and history for the conflicting disk settings.
    pub fn use_disk(&mut self) {
        if let Some(conflict) = self.conflict.take() {
            self.adopt_disk(conflict.overrides, conflict.revision);
        }
    }

    /// Mark a save as started and return the settings to submit.
    pub fn begin_save(&mut self) -> PostProcessOverrides {
        self.saving = true;
        self.draft.clone()
    }

    /// Mark a save as finished, successful or not.
    pub fn end_save(&mut self) {
        self.saving = false;
    }

    /// Record a successful save of `submitted` at `revision`; edits made while saving stay in the draft.
    pub fn mark_saved(&mut self, submitted: PostProcessOverrides, revision: &str) {
        self.saving = false;
        self.saved = submitted;
        self.revision = revision.to_owned();
        if self.conflict.as_ref().is_some_and(|conflict| conflict.revision == revision) {
            self.conflict = None;
        }
    }

    fn replace_draft(&mut self, overrides: PostProcessOverrides) {
        if overrides.same_values(&self.draft) {
            return;
        }
        self.undo_history.push(std::mem::replace(&mut self.draft, overrides));
        if self.undo_history.len() > MAXIMUM_HISTORY_LENGTH {
            self.undo_history.remove(0);
        }
        self.redo_history.clear();
    }

    fn adopt_disk(&mut self, overrides: PostProcessOverrides, revision: String) {
        self.saved = overrides.clone();
        self.draft = overrides;
        self.revision = revision;
        self.conflict = None;
        self.undo_history.clear();
        self.redo_history.clear();
    }
}

#[cfg(test)]
#[path = "post_process_session.test.rs"]
mod tests;
