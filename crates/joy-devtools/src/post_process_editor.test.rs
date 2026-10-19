// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/post_process_editor.rs`.

use super::*;
use joy_renderer::post_process::ToneMapper;
use serde_json::json;

fn level() -> Value {
    json!({"version": 1, "name": "Test", "entities": [
        {"id": "room", "transform": {"position": [0, 0, 0]}, "components": {"postProcess": {"shape": "box", "halfExtents": [2, 2, 2], "priority": 5, "exposure": 3}}},
        {"id": "look", "components": {"postProcess": {"unbounded": true, "exposure": 2}}}
    ]})
}

fn editor() -> PostProcessEditor {
    PostProcessEditor::new(PostProcessEditorOptions { title: "Test".into(), level: level(), ..Default::default() })
        .unwrap()
}

fn volumes() -> PostProcessVolumes {
    let level = joy_gameplay::LevelData::from_json(&level()).unwrap();
    PostProcessVolumes::from_level(&level).unwrap()
}

#[test]
fn previews_resolve_the_draft_without_changing_it() {
    let mut editor = editor();
    assert_eq!(editor.entity(), "look", "the global volume is edited by default");
    let mut volumes = volumes();
    let outside = Vec3::new(10.0, 0.0, 0.0);
    assert_eq!(editor.preview(&mut volumes, outside).exposure, 2.0);
    assert_eq!(editor.preview(&mut volumes, Vec3::ZERO).exposure, 3.0, "the room volume applies on top");

    let context = Context::default();
    editor.perform(&context, Action::Set("exposure", PostProcessValue::Number(1.5)));
    assert_eq!(editor.preview(&mut volumes, outside).exposure, 1.5);
    editor.perform(&context, Action::CompareSaved(true));
    assert_eq!(editor.preview(&mut volumes, outside).exposure, 2.0);
    editor.perform(&context, Action::Bypass(true));
    let bypassed = editor.preview(&mut volumes, outside);
    assert_eq!((bypassed.tone_mapper, bypassed.antialias_strength, bypassed.exposure), (ToneMapper::None, 0.0, 1.0));
    assert_eq!(editor.session().settings().exposure, 1.5);
    assert!(!editor.can_save());
}

#[test]
fn invalid_imports_and_edits_report_errors_without_changing_the_draft() {
    let mut editor = editor();
    let context = Context::default();
    editor.import_text = "{".to_owned();
    editor.perform(&context, Action::Import);
    assert!(editor.error.contains("not valid JSON"));
    editor.import_text = json!({"glow": 1}).to_string();
    editor.perform(&context, Action::Import);
    assert!(!editor.error.is_empty());
    editor.perform(&context, Action::Set("exposure", PostProcessValue::Number(-1.0)));
    assert!(!editor.error.is_empty());
    assert_eq!(editor.session().settings().exposure, 2.0);
    editor.import_text = json!({"unbounded": false, "saturation": 1.2}).to_string();
    editor.perform(&context, Action::Import);
    assert!(editor.error.is_empty());
    assert_eq!(editor.session().draft().record().keys().collect::<Vec<_>>(), ["saturation"]);
}

#[test]
fn levels_without_a_global_volume_cannot_be_edited_unless_one_is_named() {
    let mut level = level();
    level["entities"].as_array_mut().unwrap().pop();
    let options = |entity: Option<&str>| PostProcessEditorOptions {
        level: level.clone(),
        entity: entity.map(str::to_owned),
        ..Default::default()
    };
    assert!(matches!(PostProcessEditor::new(options(None)), Err(PostProcessEditorError::NoVolume(_))));
    assert_eq!(PostProcessEditor::new(options(Some("room"))).unwrap().entity(), "room");
}

#[test]
fn drawing_the_panel_does_not_edit_the_draft() {
    let mut editor = editor();
    let context = Context::default();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
        ..Default::default()
    };
    dock::set_open(&context, DockTool::PostProcessing, true);
    for _ in 0..3 {
        let mut output = context.run_ui(input(), |ui| editor.ui(ui.ctx()));
        output.textures_delta.clear();
    }
    assert_eq!(dock::open_tool(&context), Some(DockTool::PostProcessing));
    assert!(!editor.session().dirty() && !editor.session().can_undo(), "{:?}", editor.session().draft());
}

#[test]
fn the_render_clock_holds_while_paused() {
    let mut editor = editor();
    assert_eq!(editor.render_time(0.5), 0.5);
    editor.toggle_paused();
    assert_eq!(editor.render_time(0.5), 0.5);
    assert!(editor.paused());
}
