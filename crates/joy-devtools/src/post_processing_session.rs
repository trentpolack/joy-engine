// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One editing session over a saved post-processing profile: draft edits with undo and redo, a saved baseline, and
//! conflicts with external file changes.

use serde_json::Value;

use joy_core::format::json_number;
use joy_core::{
    PostProcessingConfig,
    PostProcessingError,
    PostProcessingProfile,
};

/// Undo steps retained per session.
const MAXIMUM_HISTORY_LENGTH: usize = 100;

/// A new value for one schema control.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    /// A numeric control's value.
    Number(f64),
    /// An enumerated control's spelling.
    Choice(String),
}

impl ControlValue {
    /// The JSON value stored in a profile; integral numbers stay integers, as JavaScript writes them.
    fn to_json(&self) -> Value {
        match self {
            Self::Number(number) => json_number(*number),
            Self::Choice(choice) => Value::String(choice.clone()),
        }
    }
}

/// An external profile and revision that arrived while the draft had unsaved edits.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileConflict {
    /// The profile on disk.
    pub profile: PostProcessingProfile,
    /// The disk revision.
    pub revision: String,
}

/// Draft edits, history, and the saved baseline for one editor.
#[derive(Clone, Debug)]
pub struct PostProcessingSession {
    saved: PostProcessingProfile,
    draft: PostProcessingProfile,
    revision: String,
    saving: bool,
    conflict: Option<ProfileConflict>,
    undo_history: Vec<PostProcessingProfile>,
    redo_history: Vec<PostProcessingProfile>,
}

impl PostProcessingSession {
    /// Start from authored profile data at a revision (an opaque content identity, or empty when unknown).
    pub fn new(profile: &Value, revision: impl Into<String>) -> Result<Self, PostProcessingError> {
        let profile = PostProcessingProfile::validate(profile)?;
        Ok(Self {
            saved: profile.clone(),
            draft: profile,
            revision: revision.into(),
            saving: false,
            conflict: None,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
        })
    }

    /// The edited profile.
    pub fn draft(&self) -> &PostProcessingProfile {
        &self.draft
    }

    /// The last saved or loaded profile.
    pub fn saved(&self) -> &PostProcessingProfile {
        &self.saved
    }

    /// The draft resolved into complete display settings.
    pub fn config(&self) -> PostProcessingConfig {
        resolve(&self.draft)
    }

    /// The saved profile resolved into complete display settings.
    pub fn saved_config(&self) -> PostProcessingConfig {
        resolve(&self.saved)
    }

    /// The revision the saved profile came from.
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Whether the draft differs from the saved profile.
    pub fn dirty(&self) -> bool {
        !equal_profiles(&self.draft, &self.saved)
    }

    /// Whether a save is in flight.
    pub fn saving(&self) -> bool {
        self.saving
    }

    /// An external change that needs [`keep_local`](Self::keep_local) or [`use_disk`](Self::use_disk).
    pub fn conflict(&self) -> Option<&ProfileConflict> {
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

    /// Replace the draft with validated profile data as one undoable step; invalid data leaves the session unchanged.
    pub fn replace(&mut self, profile: &Value) -> Result<(), PostProcessingError> {
        let validated = PostProcessingProfile::validate(profile)?;
        self.replace_profile(validated);
        Ok(())
    }

    /// Set one control's override.
    pub fn set_value(&mut self, key: &str, value: ControlValue) -> Result<(), PostProcessingError> {
        let mut profile = profile_json(&self.draft);
        profile["overrides"][key] = value.to_json();
        self.replace(&profile)
    }

    /// Remove one control's override so it inherits from the preset again.
    pub fn reset_value(&mut self, key: &str) {
        let mut profile = self.draft.clone();
        if profile.overrides.remove(key).is_some() {
            self.replace_profile(profile);
        }
    }

    /// Start over from a preset with no overrides.
    pub fn select_preset(&mut self, preset: &str) -> Result<(), PostProcessingError> {
        self.replace(&serde_json::json!({ "version": 1, "preset": preset, "overrides": {} }))
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

    /// Return the draft to the saved profile as one undoable step.
    pub fn revert(&mut self) {
        self.replace_profile(self.saved.clone());
    }

    /// Receive the profile on disk. A clean draft adopts it; unsaved edits that differ raise a conflict instead. The
    /// revision this session already holds clears any conflict (an echo of its own save, or a restored file).
    pub fn receive_external(&mut self, profile: &Value, revision: &str) -> Result<(), PostProcessingError> {
        let validated = PostProcessingProfile::validate(profile)?;
        if revision == self.revision {
            self.conflict = None;
            return Ok(());
        }
        if (self.dirty() || self.saving) && !equal_profiles(&validated, &self.draft) {
            self.conflict = Some(ProfileConflict { profile: validated, revision: revision.to_owned() });
            return Ok(());
        }
        self.adopt_disk(validated, revision.to_owned());
        Ok(())
    }

    /// Keep the draft and treat the conflicting disk profile as the saved baseline.
    pub fn keep_local(&mut self) {
        if let Some(conflict) = self.conflict.take() {
            self.saved = conflict.profile;
            self.revision = conflict.revision;
        }
    }

    /// Discard the draft and history for the conflicting disk profile.
    pub fn use_disk(&mut self) {
        if let Some(conflict) = self.conflict.take() {
            self.adopt_disk(conflict.profile, conflict.revision);
        }
    }

    /// Mark a save as started and return the profile to submit.
    pub fn begin_save(&mut self) -> PostProcessingProfile {
        self.saving = true;
        self.draft.clone()
    }

    /// Mark a save as finished, successful or not.
    pub fn end_save(&mut self) {
        self.saving = false;
    }

    /// Record a successful save of `submitted` at `revision`; edits made while saving stay in the draft.
    pub fn mark_saved(&mut self, submitted: PostProcessingProfile, revision: &str) {
        self.saving = false;
        self.saved = submitted;
        self.revision = revision.to_owned();
        if self.conflict.as_ref().is_some_and(|conflict| conflict.revision == revision) {
            self.conflict = None;
        }
    }

    fn replace_profile(&mut self, profile: PostProcessingProfile) {
        if equal_profiles(&profile, &self.draft) {
            return;
        }
        self.undo_history.push(std::mem::replace(&mut self.draft, profile));
        if self.undo_history.len() > MAXIMUM_HISTORY_LENGTH {
            self.undo_history.remove(0);
        }
        self.redo_history.clear();
    }

    fn adopt_disk(&mut self, profile: PostProcessingProfile, revision: String) {
        self.saved = profile.clone();
        self.draft = profile;
        self.revision = revision;
        self.conflict = None;
        self.undo_history.clear();
        self.redo_history.clear();
    }
}

/// A profile as JSON data.
pub(crate) fn profile_json(profile: &PostProcessingProfile) -> Value {
    serde_json::json!({ "version": profile.version, "preset": profile.preset, "overrides": profile.overrides })
}

/// Resolve a validated profile; validation guarantees it resolves, and engine defaults stand in otherwise.
fn resolve(profile: &PostProcessingProfile) -> PostProcessingConfig {
    profile.resolve().unwrap_or_default()
}

/// Profiles match when presets match and every override is equal, comparing numbers by value (`2` equals `2.0`).
fn equal_profiles(left: &PostProcessingProfile, right: &PostProcessingProfile) -> bool {
    left.version == right.version
        && left.preset == right.preset
        && left.overrides.len() == right.overrides.len()
        && left.overrides.iter().all(|(key, value)| {
            right.overrides.get(key).is_some_and(|other| match (value.as_f64(), other.as_f64()) {
                (Some(left), Some(right)) => left == right,
                _ => value == other,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(overrides: Value) -> Value {
        json!({ "version": 1, "preset": "default", "overrides": overrides })
    }

    fn session(overrides: Value, revision: &str) -> PostProcessingSession {
        PostProcessingSession::new(&profile(overrides), revision).unwrap()
    }

    fn exposure(session: &PostProcessingSession) -> f32 {
        session.config().exposure
    }

    #[test]
    fn reset_undo_redo_and_revert_preserve_saved_values() {
        let mut session = session(json!({ "exposure": 2 }), "a");
        session.set_value("exposure", ControlValue::Number(3.0)).unwrap();
        assert!(session.dirty());
        session.reset_value("exposure");
        assert_eq!(exposure(&session), 1.0);
        session.undo();
        assert_eq!(exposure(&session), 3.0);
        session.redo();
        assert_eq!(exposure(&session), 1.0);
        session.revert();
        assert_eq!(exposure(&session), 2.0);
        assert!(!session.dirty());
    }

    #[test]
    fn invalid_edits_are_atomic_and_clean_disk_updates_apply_immediately() {
        let mut session = session(json!({}), "a");
        assert!(session.set_value("exposure", ControlValue::Number(-1.0)).is_err());
        assert_eq!(exposure(&session), 1.0);
        assert!(!session.can_undo());
        session.receive_external(&profile(json!({ "exposure": 4 })), "b").unwrap();
        assert_eq!(exposure(&session), 4.0);
        assert_eq!(session.revision(), "b");
        assert!(!session.dirty());
    }

    #[test]
    fn dirty_external_conflicts_require_an_explicit_choice() {
        let mut session = session(json!({}), "a");
        session.set_value("exposure", ControlValue::Number(3.0)).unwrap();
        session.receive_external(&profile(json!({ "exposure": 4 })), "b").unwrap();
        assert_eq!(exposure(&session), 3.0);
        assert!(session.conflict().is_some());
        session.keep_local();
        assert_eq!(session.revision(), "b");
        assert!(session.dirty());
        session.receive_external(&profile(json!({ "exposure": 5 })), "c").unwrap();
        session.use_disk();
        assert_eq!(exposure(&session), 5.0);
        assert!(!session.dirty());
    }

    #[test]
    fn save_completion_preserves_edits_made_while_saving_and_clears_echoes() {
        let mut session = session(json!({}), "a");
        session.set_value("exposure", ControlValue::Number(2.0)).unwrap();
        let submitted = session.draft().clone();
        session.set_value("exposure", ControlValue::Number(3.0)).unwrap();
        session.receive_external(&profile_json(&submitted), "b").unwrap();
        session.mark_saved(submitted, "b");
        assert_eq!(exposure(&session), 3.0);
        assert_eq!(session.saved().overrides["exposure"], json!(2));
        assert!(session.dirty());
        assert!(session.conflict().is_none());
    }

    #[test]
    fn reverting_while_a_save_is_pending_survives_its_watcher_echo() {
        let mut session = session(json!({ "exposure": 1 }), "a");
        session.set_value("exposure", ControlValue::Number(2.0)).unwrap();
        let submitted = session.begin_save();
        session.revert();
        session.receive_external(&profile_json(&submitted), "b").unwrap();
        session.mark_saved(submitted, "b");
        assert_eq!(exposure(&session), 1.0);
        assert_eq!(session.saved().overrides["exposure"], json!(2));
        assert!(session.dirty());
    }

    #[test]
    fn restoring_the_original_disk_revision_clears_an_obsolete_conflict() {
        let original = profile(json!({ "exposure": 1 }));
        let mut session = PostProcessingSession::new(&original, "a").unwrap();
        session.set_value("exposure", ControlValue::Number(3.0)).unwrap();
        session.receive_external(&profile(json!({ "exposure": 2 })), "b").unwrap();
        session.receive_external(&original, "a").unwrap();
        assert!(session.conflict().is_none());
        assert_eq!(exposure(&session), 3.0);
        assert_eq!(session.revision(), "a");
    }

    #[test]
    fn presets_clear_overrides_and_numbers_compare_by_value() {
        let mut session = session(json!({ "exposure": 2.0 }), "a");
        // `2` and `2.0` are the same edit; it neither dirties the draft nor adds history.
        session.set_value("exposure", ControlValue::Number(2.0)).unwrap();
        assert!(!session.dirty() && !session.can_undo());
        session.select_preset("soft").unwrap();
        assert_eq!(session.draft().preset, "soft");
        assert!(session.draft().overrides.is_empty());
        assert_eq!(session.config().bloom_strength, 0.25);
        assert!(session.select_preset("missing").is_err());
        session.set_value("toneMapper", ControlValue::Choice("aces".to_owned())).unwrap();
        assert_eq!(session.draft().overrides["toneMapper"], json!("aces"));
    }
}
