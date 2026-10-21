// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The session without UI or GPU: the compile lifecycle's status lines, property history, variants, files, and the
//! replacement protection a source-only save keeps.

use form_editor::document::FormDocument;
use form_editor::session::{
    PreviewState,
    SaveFormat,
    Session,
    SessionEvent,
    StatusState,
};
use joy_engine::serde_json::json;

const SOURCE: &str = "param height = 2 meta(min=0, max=5, step=0.5)\noutput(\"Tower\", translate(box(1, height, 1), vector(0, height/2, 0)))\n";

/// A session for SOURCE with its first build accepted.
fn built() -> Session {
    let mut session = Session::new(FormDocument::from_script("Tower", SOURCE), None);
    session.compile();
    accept(&mut session);
    session
}

/// Finish the outstanding build and accept it as the preview would.
fn accept(session: &mut Session) {
    match session.finish_build() {
        Some(SessionEvent::BuildReady {
            data,
            milliseconds,
            ..
        }) => session.accept_build(*data, milliseconds),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn builds_report_progress_success_and_failure() {
    let mut session = Session::new(FormDocument::from_script("Tower", SOURCE), None);
    session.compile();
    assert_eq!(session.status().message, "Starting compiler…");
    assert_eq!(session.preview_text(), "Preview pending · no accepted geometry");
    accept(&mut session);
    assert_eq!(session.status().message, "Built successfully · 8 vertices");
    assert_eq!(session.preview_state(), PreviewState::Accepted);
    assert_eq!(session.parameters().len(), 1);
    assert_eq!(session.statistics().vertices, 8);

    session.source = "box(".into();
    session.source_changed();
    assert_eq!(session.status().message, "Waiting for your next idea…");
    assert!(session.rebuild_scheduled());
    match session.finish_build() {
        Some(SessionEvent::BuildRejected(diagnostic)) => assert_eq!(diagnostic.line, 1),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(session.status().state, StatusState::Error);
    assert!(session.error_location().is_some());
    assert_eq!(session.preview_text(), "Preview failed · last accepted geometry");
    assert_eq!(session.data().unwrap().positions.len(), 24, "the last accepted build stays");

    session.set_live(false);
    assert_eq!(session.status().message, "Live preview paused. Run to update.");
    session.source = SOURCE.into();
    session.source_changed();
    assert_eq!(session.status().message, "Edited · Run to update the preview.");
    assert!(!session.rebuild_scheduled());
}

#[test]
fn property_history_undoes_overrides_and_restarts_after_script_edits() {
    let mut session = built();
    session.set_override("height", Some(json!(4)));
    session.commit_properties();
    session.set_override("height", Some(json!(5)));
    session.commit_properties();
    assert!(session.undo_properties());
    assert_eq!(session.overrides()["height"], json!(4));
    assert!(session.undo_properties());
    assert!(session.overrides().is_empty());
    assert!(session.redo_properties());
    assert_eq!(session.overrides()["height"], json!(4));
    session.reset_parameter("height");
    assert!(session.overrides().is_empty());
    assert!(session.undo_properties(), "resets participate in property undo");
    assert_eq!(session.overrides()["height"], json!(4));
    session.source.push_str("# edited\n");
    session.source_changed();
    assert!(!session.can_undo_properties(), "script edits restart property history");
    // Imported meshes stay outside history snapshots.
    session.set_override("piece", Some(form_editor::parameter_values::MeshPreset::Box.value()));
    assert!(!session.property_snapshot().contains_key("piece"));
}

#[test]
fn variants_save_apply_rename_and_delete_with_validation() {
    let mut session = built();
    session.set_override("height", Some(json!(4.5)));
    assert_eq!(session.save_variant("Tall").unwrap(), 0);
    assert!(session.save_variant(" tall ").unwrap_err().contains("unique"));
    assert!(session.save_variant("").is_err());
    session.set_override("height", Some(json!(1)));
    session.save_variant("Short").unwrap();
    assert_eq!(session.apply_variant(0).unwrap(), "Tall");
    assert_eq!(session.overrides()["height"], json!(4.5));
    session.rename_variant(1, "Squat").unwrap();
    assert_eq!(session.variants()[1].name, "Squat");
    session.delete_variant(0).unwrap();
    assert_eq!(session.variants().len(), 1);
    assert!(session.apply_variant(4).is_err());
    let document: joy_engine::serde_json::Value =
        joy_engine::serde_json::from_str(&session.save_contents(SaveFormat::Instance).unwrap()).unwrap();
    assert_eq!(document["variants"][0]["name"], "Squat");
}

#[test]
fn source_only_saves_keep_replacement_protection_while_inspector_data_would_be_lost() {
    let mut session = built();
    session.source.push_str("# edited\n");
    session.source_changed();
    assert!(session.has_edits());
    let script = session.save_contents(SaveFormat::Form).unwrap();
    session.save_completed(SaveFormat::Form, &script, true);
    assert!(!session.has_edits(), "a script-only study is fully saved by .form");
    assert_eq!(session.save_state().unwrap().0, "FORM script saved");

    session.set_override("height", Some(json!(3)));
    let script = session.save_contents(SaveFormat::Form).unwrap();
    assert_eq!(script, session.source, ".form never bakes overrides");
    session.save_completed(SaveFormat::Form, &script, false);
    assert!(session.has_edits(), "overrides would be lost, so protection stays");
    assert_eq!(session.save_state().unwrap().0, "FORM script download requested");
    let document = session.save_contents(SaveFormat::Instance).unwrap();
    session.save_completed(SaveFormat::Instance, &document, true);
    assert!(!session.has_edits());
}

#[test]
fn opened_files_choose_their_format_by_extension() {
    let slab = "output(\"Slab\", box(1, 1, 1))";
    let script = Session::read_file("notes.txt", slab).unwrap();
    assert_eq!((script.name.as_str(), script.source.as_str()), ("notes", slab));
    let raw = Session::read_file("Tower.FORM", slab).unwrap();
    assert_eq!(raw.name, "Tower");
    assert!(Session::read_file("tower.forminst", "box(").is_err(), "instances must be JSON");
    assert!(Session::read_file("tower.formlab", "box(").is_err(), "version 1 documents must be JSON");
    let legacy = format!("{{\"version\":1,\"name\":\"Old\",\"source\":{}}}", joy_engine::serde_json::json!(slab));
    assert_eq!(Session::read_file("old.formlab", &legacy).unwrap().source, slab, "version 1 documents open packed");
    let long = "#".repeat(200_001);
    assert!(Session::read_file("long.form", &long).unwrap_err().to_string().contains("200,000"));
}

#[test]
fn renaming_edits_project_metadata_or_explains_why_not() {
    let mut session = built();
    session.rename("Spire").unwrap();
    assert!(session.source.starts_with("project meta(name=\"Spire\")\n"));
    assert_eq!(session.name, "Spire");
    session.source = "box(".into();
    assert!(session.rename("Other").unwrap_err().starts_with("Fix the script before renaming its project metadata"));
    assert_eq!(session.name, "Spire");
}
