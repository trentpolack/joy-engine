// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/viewport_input.rs`.

use super::*;
use egui::{
    pos2,
    vec2,
};

fn frame(context: &egui::Context, events: Vec<Event>) -> ViewportActions {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
        events,
        ..Default::default()
    };
    let mut result = None;
    let mut output = context.run_ui(input, |root| {
        result =
            Some(viewport_input(root, Rect::from_min_size(pos2(50.0, 50.0), vec2(300.0, 200.0)), Id::new("viewport")));
    });
    output.textures_delta.clear();
    result.unwrap()
}

fn button(pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers }
}

#[test]
fn drags_orbit_or_pan_wheels_zoom_and_focused_keys_shortcut() {
    let context = egui::Context::default();
    let start = pos2(200.0, 150.0);
    frame(&context, vec![Event::PointerMoved(start)]);
    frame(&context, vec![button(start, PointerButton::Primary, true, Modifiers::NONE)]);
    let mut drags = Vec::new();
    for step in 1..=3 {
        let actions = frame(&context, vec![Event::PointerMoved(start + vec2(10.0 * step as f32, 0.0))]);
        drags.extend(actions.drag);
    }
    assert!(drags.iter().all(|(mode, _)| *mode == ViewportGesture::Orbit));
    assert!((drags.iter().map(|(_, delta)| delta.x).sum::<f32>() - 30.0).abs() < 0.01);
    let released =
        frame(&context, vec![button(start + vec2(30.0, 0.0), PointerButton::Primary, false, Modifiers::NONE)]);
    assert!(released.click.is_none(), "a drag is not a click");
    assert!(released.response.has_focus(), "pressing focuses the viewport");

    // Secondary drags pan.
    frame(&context, vec![button(start, PointerButton::Secondary, true, Modifiers::NONE)]);
    let panned = frame(&context, vec![Event::PointerMoved(start + vec2(0.0, 20.0))]);
    assert_eq!(panned.drag.map(|(mode, _)| mode), Some(ViewportGesture::Pan));
    frame(&context, vec![button(start + vec2(0.0, 20.0), PointerButton::Secondary, false, Modifiers::NONE)]);

    let wheel = frame(
        &context,
        vec![Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, -2.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(wheel.zoom, 32.0, "two lines toward the author zoom out by 32 page pixels");
    let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    let keys = frame(&context, vec![key(Key::F), key(Key::ArrowLeft), key(Key::Num0)]);
    assert_eq!(keys.shortcuts, vec![ViewportShortcut::Frame, ViewportShortcut::Reset, ViewportShortcut::Left]);

    let clicked = frame(
        &context,
        vec![
            button(start, PointerButton::Primary, true, Modifiers::NONE),
            button(start, PointerButton::Primary, false, Modifiers::NONE),
        ],
    );
    assert_eq!(clicked.click, Some(start));
}
