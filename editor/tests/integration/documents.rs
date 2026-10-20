// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Tab documents: edits made during a save, external changes, and conflict resolution never lose unsaved text.

use joy_editor::server::DiskDocument;
use joy_editor::workspace::documents::WorkspaceDocument;

fn disk(text: &str, revision: &str) -> DiskDocument {
    DiskDocument {
        text: text.to_owned(),
        revision: revision.to_owned(),
    }
}

#[test]
fn document_conflicts_and_edits_during_save_retain_every_unsaved_change() {
    let mut document = WorkspaceDocument::new("demo", "tree.form", disk("original", "a"));
    document.edit("first");
    let snapshot = document.snapshot();
    document.edit("second");
    document.saved(&snapshot, &disk("first", "b"));
    assert_eq!(document.text, "second");
    assert!(document.dirty());
    assert!(!document.external(&disk("external", "c")));
    assert_eq!(document.text, "second");
    assert_eq!(document.conflict.as_ref().map(|conflict| conflict.text.as_str()), Some("external"));
    document.keep_local();
    assert_eq!(document.revision, "c");
    assert_eq!(document.text, "second");
    let snapshot = document.snapshot();
    document.saved(&snapshot, &disk("second", "d"));
    assert!(!document.dirty());
    assert!(document.external(&disk("VS Code", "e")));
    assert_eq!(document.text, "VS Code");
    document.edit("local");
    document.external(&disk("disk", "f"));
    document.reload_disk();
    assert_eq!(document.text, "disk");
    assert!(!document.dirty());

    // A save in progress ignores polling; an unchanged revision is not an external edit.
    document.saving = true;
    assert!(!document.external(&disk("other", "g")));
    document.saving = false;
    assert!(!document.external(&disk("disk", "f")));
}
