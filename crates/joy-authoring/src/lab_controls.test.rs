// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/lab_controls.rs`.

use super::*;

/// Run one UI pass with `events`, drawing `contents` in a central panel.
fn pass(context: &egui::Context, events: Vec<egui::Event>, mut contents: impl FnMut(&mut Ui)) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(480.0, 320.0))),
        events,
        ..Default::default()
    };
    let mut output = context.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| contents(ui));
    });
    output.textures_delta.clear();
}

fn click(position: Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ]
}

fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Draw one field, returning its response and rect.
fn field(
    context: &egui::Context,
    events: Vec<egui::Event>,
    value: &mut f64,
    drafts: &mut NumericDrafts,
) -> ControlResponse {
    let mut result = None;
    pass(context, events, |ui| {
        result = Some(NumberField::new("power", value).range(0.0, 2.0).step(0.1).show(ui, drafts));
    });
    result.unwrap()
}

#[test]
fn rejected_drafts_stay_visible_until_repaired_and_never_change_the_value() {
    let context = egui::Context::default();
    let mut drafts = NumericDrafts::default();
    let mut value = 1.25;
    let first = field(&context, vec![], &mut value, &mut drafts);
    let center = first.response.rect.center();
    field(&context, click(center), &mut value, &mut drafts);
    // Typing replaces the selected value; Enter confirms an out-of-range draft.
    field(&context, vec![egui::Event::Text("5".to_owned())], &mut value, &mut drafts);
    assert!(drafts.has_rejected(), "an invalid draft being typed already blocks actions");
    field(&context, vec![key(Key::Enter, Default::default())], &mut value, &mut drafts);
    assert_eq!(value, 1.25, "the accepted value is unchanged");
    let rejected = drafts.messages();
    assert_eq!(rejected, vec!["Value must be less than or equal to 2."]);
    assert!(drafts.require_valid().is_err());
    // Clicking elsewhere keeps the draft visible.
    field(&context, click(pos2(400.0, 300.0)), &mut value, &mut drafts);
    field(&context, vec![], &mut value, &mut drafts);
    let id = drafts.rejected.keys().next().copied().unwrap();
    assert_eq!(drafts.rejected(id).map(|draft| draft.text.as_str()), Some("5"));
    // Repairing it commits once.
    field(&context, click(center), &mut value, &mut drafts);
    field(&context, vec![egui::Event::Text("1.5".to_owned())], &mut value, &mut drafts);
    let committed = field(&context, vec![key(Key::Enter, Default::default())], &mut value, &mut drafts);
    assert_eq!(value, 1.5);
    assert!(committed.changed && committed.committed());
    assert!(!drafts.has_rejected());
}

#[test]
fn arrow_keys_step_with_shift_fine_and_alt_coarse() {
    let context = egui::Context::default();
    let mut drafts = NumericDrafts::default();
    let mut value = 1.0;
    let center = field(&context, vec![], &mut value, &mut drafts).response.rect.center();
    field(&context, click(center), &mut value, &mut drafts);
    let up = field(&context, vec![key(Key::ArrowUp, Default::default())], &mut value, &mut drafts);
    assert_eq!(value, 1.1);
    assert!(up.committed(), "each step commits");
    field(&context, vec![key(Key::ArrowUp, egui::Modifiers::SHIFT)], &mut value, &mut drafts);
    assert_eq!(value, 1.11);
    field(&context, vec![key(Key::ArrowDown, egui::Modifiers::ALT)], &mut value, &mut drafts);
    assert_eq!(value, 0.11);
    field(&context, vec![key(Key::ArrowDown, egui::Modifiers::ALT)], &mut value, &mut drafts);
    assert_eq!(value, 0.0, "steps clamp to the bounds");
}

#[test]
fn scrubbing_moves_continuously_and_commits_once_on_release() {
    let context = egui::Context::default();
    let mut drafts = NumericDrafts::default();
    let mut value = 1.0;
    let center = field(&context, vec![], &mut value, &mut drafts).response.rect.center();
    let press = egui::Event::PointerButton {
        pos: center,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    };
    field(&context, vec![egui::Event::PointerMoved(center), press], &mut value, &mut drafts);
    let mut phases = Vec::new();
    for offset in [10.0, 20.0] {
        let moved =
            field(&context, vec![egui::Event::PointerMoved(center + vec2(offset, 0.0))], &mut value, &mut drafts);
        phases.push(moved.phase);
    }
    assert_eq!(value, 2.0, "ten steps of 0.1 over 20 points, clamped at the maximum");
    assert_eq!(phases.last(), Some(&EditPhase::Continuing));
    let release = egui::Event::PointerButton {
        pos: center + vec2(20.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    };
    let released = field(&context, vec![release], &mut value, &mut drafts);
    assert!(released.committed());
}

#[test]
fn slider_presses_and_drags_are_one_gesture_committed_on_release() {
    let context = egui::Context::default();
    let mut drafts = NumericDrafts::default();
    let mut value = 1.0;
    let frame = |events: Vec<egui::Event>, value: &mut f64, drafts: &mut NumericDrafts| {
        let mut response = None;
        pass(&context, events, |ui| {
            ui.set_width(300.0);
            response = Some(ScalarControl::new("power", value).range(0.0, 2.0).name("power").show(ui, drafts));
        });
        response.unwrap()
    };
    let slider_rect = frame(vec![], &mut value, &mut drafts).response.rect;
    let start = pos2(slider_rect.left() + 20.0, slider_rect.center().y);
    let press = egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    };
    let pressed = frame(vec![egui::Event::PointerMoved(start), press], &mut value, &mut drafts);
    assert!(value < 1.0, "pressing jumps to the pointer: {value}");
    assert_eq!(pressed.phase, EditPhase::Continuing);
    let moved = frame(vec![egui::Event::PointerMoved(start + vec2(40.0, 0.0))], &mut value, &mut drafts);
    assert_eq!(moved.phase, EditPhase::Continuing);
    let release = egui::Event::PointerButton {
        pos: start + vec2(40.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    };
    let released = frame(vec![release], &mut value, &mut drafts);
    assert!(released.committed(), "the release commits once");
    let mut history = crate::PropertyHistory::new(1.0);
    assert!(history.commit(value));
    assert_eq!(history.undo_count(), 1);
}

#[test]
fn displaying_controls_never_rounds_or_clamps_authored_values() {
    let context = egui::Context::default();
    let mut drafts = NumericDrafts::default();
    let mut scalar = 1.234_567_890_123_456_7;
    let mut outside = 7.5;
    let mut color = [3.25, 0.123_456_789_012_345_6, -0.5, 0.42];
    let original = color;
    let mut swatch_center = Pos2::ZERO;
    for frame in 0..4 {
        let events = if frame == 1 { click(swatch_center) } else { vec![] };
        pass(&context, events, |ui| {
            ScalarControl::new("scalar", &mut scalar).range(0.0, 2.0).show(ui, &mut drafts);
            ScalarControl::new("outside", &mut outside).range(0.0, 2.0).show(ui, &mut drafts);
            let response = ColorControl::new("tint", &mut color, &[0.0; 4], &[1.0; 4]).show(ui, &drafts);
            swatch_center = response.response.rect.center();
        });
    }
    let popup_open = egui::Popup::is_any_open(&context);
    assert!(popup_open, "the picker opened");
    assert_eq!(scalar.to_bits(), 1.234_567_890_123_456_7_f64.to_bits());
    assert_eq!(outside, 7.5, "sliders do not clamp values they did not edit");
    assert_eq!(color.map(f64::to_bits), original.map(f64::to_bits), "opening the picker writes nothing");
}

#[test]
fn resets_restore_defaults_and_rejected_drafts_block_picker_choices() {
    let context = egui::Context::default();
    let mut drafts = NumericDrafts::default();
    let mut value = 1.5;
    let mut reset_rect = Rect::NOTHING;
    pass(&context, vec![], |ui| {
        ScalarControl::new("power", &mut value).range(0.0, 2.0).default_value(Some(1.0)).show(ui, &mut drafts);
        reset_rect = ui.min_rect();
    });
    assert!(is_modified(value, Some(1.0)) && !is_modified(1.0, Some(1.0)) && !is_modified(1.0, None));
    // The reset button is the last widget in the row; click near the row's right end.
    let mut response = None;
    let row_right = pos2(reset_rect.right() - (RESET_BUTTON_SIZE * 0.5), reset_rect.top() + 15.0);
    for events in [vec![egui::Event::PointerMoved(row_right)], click(row_right)] {
        pass(&context, events, |ui| {
            response = Some(
                ScalarControl::new("power", &mut value).range(0.0, 2.0).default_value(Some(1.0)).show(ui, &mut drafts),
            );
        });
    }
    assert_eq!(value, 1.0);
    assert!(response.unwrap().committed());

    drafts.reject(Id::new("elsewhere"), "abc".to_owned(), "Enter a finite number.".to_owned(), true);
    let mut values = [0.5, 0.5, 0.5, 1.0];
    let memory = &mut PickerMemory {
        hsva: picker_color(&values),
        hex: "#000000".to_owned(),
        hex_error: None,
        dragging: false,
    };
    let mut result = (true, EditPhase::Committed);
    pass(&context, vec![], |ui| {
        result = picker_contents(ui, &mut values, &[0.0; 4], &[1.0; 4], &drafts, memory);
    });
    assert_eq!(result, (false, EditPhase::Idle));
    assert_eq!(values, [0.5, 0.5, 0.5, 1.0]);
}
