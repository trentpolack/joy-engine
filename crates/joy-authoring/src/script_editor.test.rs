// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/script_editor.rs`.

use super::*;

struct Harness {
    context: egui::Context,
    editor: ScriptEditor,
    text: String,
}

impl Harness {
    fn new(editor: ScriptEditor, text: &str) -> Self {
        Self { context: egui::Context::default(), editor, text: text.to_owned() }
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> ScriptEditorResponse {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 360.0))),
            events,
            ..Default::default()
        };
        let mut result = None;
        let (editor, text) = (&mut self.editor, &mut self.text);
        let mut output = self.context.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| result = Some(editor.show(ui, text)));
        });
        output.textures_delta.clear();
        result.expect("the editor drew")
    }

    fn cursor(&self) -> usize {
        egui::text_edit::TextEditState::load(&self.context, self.editor.id().with("text"))
            .and_then(|state| state.cursor.char_range())
            .map_or(0, |range| range.primary.index.0)
    }
}

fn key(key: Key, modifiers: Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn text(value: &str) -> egui::Event {
    egui::Event::Text(value.to_owned())
}

#[test]
fn typing_opens_completion_and_functions_insert_calls_with_the_cursor_inside() {
    let editor = ScriptEditor::new("spawn", ScriptSyntax::particle()).with_catalog(joy_fx::particle_language_catalog());
    let mut harness = Harness::new(editor, "size = ");
    harness.editor.go_to_line(1);
    harness.frame(vec![]);
    harness.frame(vec![key(Key::End, Modifiers::NONE)]);
    harness.frame(vec![text("sm")]);
    assert!(harness.editor.completion_open(), "typing a word opens completion");
    assert_eq!(harness.editor.completion_labels()[0], "smoothstep");
    let accepted = harness.frame(vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(accepted.changed);
    assert_eq!(harness.text, "size = smoothstep()");
    harness.frame(vec![]);
    assert_eq!(harness.cursor(), 18, "the cursor sits between the parentheses");
    assert!(!harness.editor.completion_open());

    // Ctrl+Space opens the full list without a prefix; Escape closes it.
    harness.frame(vec![key(Key::Space, Modifiers::CTRL)]);
    assert_eq!(harness.editor.completion_labels().len(), joy_fx::particle_language_catalog().symbols.len());
    harness.frame(vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(!harness.editor.completion_open());
    assert_eq!(harness.text, "size = smoothstep()", "requests and dismissals never edit");
}

#[test]
fn run_shortcut_go_to_line_and_status() {
    let editor = ScriptEditor::new("form", ScriptSyntax::form()).with_catalog(joy_form::form_language_catalog());
    let mut harness = Harness::new(editor, "param width = 2\nbox(width, 1, 1)\n# end");
    harness.editor.set_diagnostics(vec![ScriptDiagnostic::error(2, 5, "Unknown parameter")]);
    harness.editor.go_to_line(3);
    harness.frame(vec![]);
    let response = harness.frame(vec![]);
    assert_eq!((response.line, response.column, response.line_count), (3, 1, 3));
    assert_eq!(response.status_text(), "Ln 3, Col 1 · 3 lines");
    let run = harness.frame(vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(run.run_requested && !run.changed, "Ctrl/Cmd+Enter runs without inserting a line");
    assert_eq!(harness.editor.diagnostics().len(), 1);
    // Tab indents with two spaces.
    harness.frame(vec![key(Key::Tab, Modifiers::NONE)]);
    assert!(harness.text.ends_with("\n  # end"), "{}", harness.text);
}

#[test]
fn read_only_views_never_edit() {
    let mut editor = ScriptEditor::new("json", ScriptSyntax::json()).read_only();
    let context = egui::Context::default();
    let json = "{\n  \"version\": 1\n}";
    editor.request_focus();
    for events in [vec![], vec![text("x"), key(Key::Enter, Modifiers::NONE)]] {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let response = editor.show_read_only(ui, json);
                assert!(!response.changed);
            });
        });
        output.textures_delta.clear();
    }
    assert!(!editor.completion_open());
}
