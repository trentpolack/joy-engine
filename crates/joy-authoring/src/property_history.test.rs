// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/property_history.rs`.

use super::*;

#[test]
fn a_drag_coalesces_into_one_entry_from_its_starting_value() {
    let mut history = PropertyHistory::new(1.0);
    // A drag moves through 1.1 and 1.2 without committing; only the release at 1.3 commits.
    assert!(history.commit(1.3));
    assert!(!history.commit(1.3), "an unchanged commit records nothing");
    assert_eq!(history.undo_count(), 1);
    assert_eq!(history.undo(&1.3), Some(1.0));
    assert_eq!(history.redo(&1.0), Some(1.3));
    assert!(!history.can_redo());
}

#[test]
fn new_edits_clear_redo_and_old_entries_drop_beyond_the_limit() {
    let mut history = PropertyHistory::new(0).with_limit(3);
    for value in 1..=5 {
        history.commit(value);
    }
    assert_eq!(history.undo_count(), 3);
    assert_eq!(history.undo(&5), Some(4));
    assert!(history.can_redo());
    history.commit(9);
    assert!(!history.can_redo(), "a new edit discards the redo branch");
    assert_eq!(history.undo(&9), Some(4));
    assert_eq!(history.undo(&4), Some(3));
    assert_eq!(history.undo(&3), Some(2));
    assert_eq!(history.undo(&2), None, "entries beyond the limit were dropped");
}

#[test]
fn unrecorded_changes_invalidate_undo_instead_of_overwriting_them() {
    let mut history = PropertyHistory::new("a".to_owned());
    history.commit("b".to_owned());
    // A structural edit moves the document on without an entry.
    history.rebase("b+parameter".to_owned());
    assert_eq!(history.undo(&"b+parameter".to_owned()), None);
    assert!(!history.can_undo(), "the stale entry is cleared");
    assert!(history.commit("c".to_owned()));
    assert_eq!(history.undo(&"c".to_owned()), Some("b+parameter".to_owned()));
    assert_eq!(history.baseline(), Some(&"b+parameter".to_owned()));
    history.reset("opened".to_owned());
    assert!(!history.can_undo() && !history.can_redo());
}
