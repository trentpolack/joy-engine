// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/config_panel.rs`.

use super::*;
use crate::config_session::tests::fields;

fn run(context: &Context, mut ui: impl FnMut(&Context)) {
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            ..Default::default()
        },
        |root| ui(root.ctx()),
    );
    output.textures_delta.clear();
}

#[test]
fn drawing_the_panel_does_not_edit_and_search_filters_without_errors() {
    let options = ConfigEditorOptions {
        title: "Test".into(),
        fields: fields(),
        values: [("speed".to_owned(), 5.5)].into_iter().collect(),
        file: None,
        restart_label: Some("Restart Scenario".into()),
    };
    let mut editor = ConfigEditor::new(options).unwrap();
    let context = Context::default();
    dock::set_open(&context, DockTool::Config, true);
    for search in ["", "spawn", "nothing matches"] {
        editor.panel.search = search.to_owned();
        run(&context, |context| editor.ui(context));
    }
    assert!(!editor.session().dirty() && !editor.session().can_undo());
    assert_eq!(editor.take_change(), None);
    assert!(!editor.take_restart());
    assert_eq!(editor.values()["speed"], 5.5);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_editors_load_their_document_and_report_it_once() {
    let directory = std::env::temp_dir().join(format!("joy-config-editor-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("tuning-overrides.json");
    std::fs::write(&path, "{\"version\": 1, \"values\": {\"count\": 6}}").unwrap();
    let options = ConfigEditorOptions {
        title: "Test".into(),
        fields: fields(),
        values: IndexMap::new(),
        file: Some(path),
        restart_label: None,
    };
    let mut editor = ConfigEditor::new(options).unwrap();
    assert!(editor.session().can_save());
    assert_eq!(editor.take_change().map(|values| values["count"]), Some(6.0));
    assert_eq!(editor.take_change(), None);
    editor.session_mut().set_value("speed", 6.0).unwrap();
    editor.session_mut().save().unwrap();
    assert!(!editor.session().dirty());
    std::fs::remove_dir_all(directory).unwrap();

    let missing = ConfigEditorOptions {
        title: "Test".into(),
        fields: fields(),
        values: IndexMap::new(),
        file: Some(std::env::temp_dir().join("joy-missing-config.json")),
        restart_label: None,
    };
    let editor = ConfigEditor::new(missing).unwrap();
    assert!(!editor.session().can_save());
    assert!(editor.panel.status.contains("Cannot load config"));
}
