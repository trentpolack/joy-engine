// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/post_process_session.rs`.

use super::*;
use serde_json::json;

fn overrides(record: Value) -> PostProcessOverrides {
    PostProcessOverrides::from_record(record.as_object().unwrap()).unwrap()
}

fn session(record: Value, revision: &str) -> PostProcessSession {
    PostProcessSession::new(overrides(record), revision)
}

fn exposure(session: &PostProcessSession) -> f32 {
    session.settings().exposure
}

#[test]
fn reset_undo_redo_and_revert_preserve_saved_values() {
    let mut session = session(json!({ "exposure": 2 }), "a");
    session.set_value("exposure", PostProcessValue::Number(3.0)).unwrap();
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
    assert!(session.set_value("exposure", PostProcessValue::Number(-1.0)).is_err());
    assert_eq!(exposure(&session), 1.0);
    assert!(!session.can_undo());
    session.receive_external(overrides(json!({ "exposure": 4 })), "b");
    assert_eq!(exposure(&session), 4.0);
    assert_eq!(session.revision(), "b");
    assert!(!session.dirty());
}

#[test]
fn dirty_external_conflicts_require_an_explicit_choice() {
    let mut session = session(json!({}), "a");
    session.set_value("exposure", PostProcessValue::Number(3.0)).unwrap();
    session.receive_external(overrides(json!({ "exposure": 4 })), "b");
    assert_eq!(exposure(&session), 3.0);
    assert!(session.conflict().is_some());
    session.keep_local();
    assert_eq!(session.revision(), "b");
    assert!(session.dirty());
    session.receive_external(overrides(json!({ "exposure": 5 })), "c");
    session.use_disk();
    assert_eq!(exposure(&session), 5.0);
    assert!(!session.dirty());
}

#[test]
fn saves_survive_edits_reverts_and_watcher_echoes_made_while_saving() {
    let mut session = session(json!({ "exposure": 1 }), "a");
    session.set_value("exposure", PostProcessValue::Number(2.0)).unwrap();
    let submitted = session.begin_save();
    session.set_value("exposure", PostProcessValue::Number(3.0)).unwrap();
    session.receive_external(submitted.clone(), "b");
    session.mark_saved(submitted, "b");
    assert_eq!(exposure(&session), 3.0);
    assert_eq!(session.saved().get("exposure"), Some(&json!(2)));
    assert!(session.dirty() && session.conflict().is_none());

    // Reverting during a save restores the old baseline; the echo of the save does not undo that.
    let submitted = session.begin_save();
    session.revert();
    session.receive_external(submitted.clone(), "c");
    session.mark_saved(submitted, "c");
    assert_eq!(exposure(&session), 2.0);
    assert_eq!(session.saved().get("exposure"), Some(&json!(3)));
}

#[test]
fn numbers_compare_by_value_and_choices_set_spellings() {
    let mut session = session(json!({ "exposure": 2.0 }), "a");
    // `2` and `2.0` are the same edit; it neither dirties the draft nor adds history.
    session.set_value("exposure", PostProcessValue::Number(2.0)).unwrap();
    assert!(!session.dirty() && !session.can_undo());
    session.set_value("toneMapper", PostProcessValue::Choice("filmic".to_owned())).unwrap();
    assert_eq!(session.draft().get("toneMapper"), Some(&json!("filmic")));
    assert!(session.set_value("glow", PostProcessValue::Number(1.0)).is_err());
}
