// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/lib.rs`.

use super::*;

fn manager() -> InputManager {
    InputManager::new(
        InputMap::new()
            .action("forward", [InputBinding::key(key::KEY_W), InputBinding::virtual_button("forward")])
            .action("reverse", [InputBinding::key(key::KEY_S)])
            .action("pause", [InputBinding::key(key::ESCAPE)])
            .action("brake", [InputBinding::key(key::SPACE), InputBinding::gamepad_button(gamepad::BUTTON_SOUTH)])
            .action("fire", [InputBinding::mouse_button(0)])
            .action_in("menu", [InputBinding::key(key::ENTER)], ["menu"])
            .axis("throttle", "forward", "reverse")
            .axis_with("steer", InputAxis::new("forward", "reverse").gamepad_axis(gamepad::AXIS_LEFT_X)),
    )
}

fn pad(axes: [f32; 4], south: f32) -> GamepadState {
    let mut buttons = vec![0.0; gamepad::BUTTON_COUNT];
    buttons[usize::from(gamepad::BUTTON_SOUTH)] = south;
    GamepadState {
        connected: true,
        axes: axes.to_vec(),
        buttons,
    }
}

#[test]
fn edges_and_axes_follow_held_controls() {
    let mut input = manager();
    input.handle_key(key::KEY_W, true);
    input.update();
    assert!(input.action("forward").pressed && input.action("forward").down);
    assert_eq!(input.axis("throttle"), 1.0);

    input.update();
    assert!(!input.action("forward").pressed && input.action("forward").down);

    input.handle_key(key::KEY_W, false);
    input.handle_key(key::KEY_S, true);
    input.update();
    assert!(input.action("forward").released);
    assert_eq!(input.axis("throttle"), -1.0);

    // Opposing digital directions cancel.
    input.handle_key(key::KEY_W, true);
    input.update();
    assert_eq!(input.axis("throttle"), 0.0);
}

#[test]
fn taps_between_frames_still_press() {
    let mut input = manager();
    input.handle_key(key::ESCAPE, true);
    input.handle_key(key::ESCAPE, false);
    input.update();
    assert!(input.pressed("pause"));
    assert!(input.action("pause").released);
    assert!(!input.action("pause").down);
    input.update();
    assert!(!input.pressed("pause"));
}

#[test]
fn virtual_buttons_touches_and_clear() {
    let mut input = manager();
    input.set_virtual("forward", 0.5);
    // Two fingers at once: on-screen controls hit-test every touch, not one pointer.
    input.handle_touch(1, Some(Vec2::new(10.0, 20.0)));
    input.handle_touch(2, Some(Vec2::new(300.0, 20.0)));
    input.handle_touch(1, Some(Vec2::new(12.0, 22.0)));
    input.update();
    assert_eq!(input.axis("throttle"), 0.5);
    assert_eq!(input.touches().collect::<Vec<_>>(), [Vec2::new(12.0, 22.0), Vec2::new(300.0, 20.0)]);
    assert_eq!(
        input.touch_points().collect::<Vec<_>>(),
        [(1, Vec2::new(12.0, 22.0)), (2, Vec2::new(300.0, 20.0))],
        "ids follow their fingers"
    );
    // Touches hold the primary pointer button, like browser pointer events.
    assert!(input.action("fire").down);
    assert_eq!(input.pointer().position, Vec2::new(12.0, 22.0));
    input.handle_touch(2, None);
    assert_eq!(input.touches().count(), 1);

    // Focus loss and pauses release everything, including fingers still down.
    input.clear();
    input.update();
    assert_eq!(input.axis("throttle"), 0.0);
    assert!(!input.action("forward").down);
    assert!(!input.action("fire").down);
    assert_eq!(input.touches().count(), 0);
}

#[test]
fn values_at_the_press_threshold_do_not_hold() {
    let mut input = manager();
    input.set_virtual("forward", PRESS_THRESHOLD);
    input.update();
    assert!(!input.action("forward").down && !input.pressed("forward"));
    assert_eq!(input.action("forward").value, PRESS_THRESHOLD);
    input.set_virtual("forward", 0.2);
    input.update();
    assert!(input.pressed("forward"));
}

#[test]
fn gamepads_rescale_dead_zones_and_wait_for_neutral_after_release() {
    let mut input = manager();
    // Analog: 0.575 with the default 0.15 dead zone rescales to 0.5; buttons count from any pad.
    input.set_gamepads(&[pad([0.575, 0.0, 0.0, 0.0], 1.0), GamepadState::default()]);
    input.update();
    assert!((input.axis("steer") - 0.5).abs() < 1e-6);
    assert!(input.pressed("brake"));
    // Sticks inside the dead zone read zero; disconnected pads contribute nothing.
    input.set_gamepads(&[
        pad([0.1, 0.0, 0.0, 0.0], 0.0),
        GamepadState {
            connected: false,
            ..pad([1.0; 4], 1.0)
        },
    ]);
    input.update();
    assert_eq!(input.axis("steer"), 0.0);
    assert!(input.action("brake").released);

    // After a release, a pad still held stays blocked until it returns to neutral.
    input.set_gamepads(&[pad([0.0; 4], 1.0)]);
    input.update();
    assert!(input.action("brake").down);
    input.clear();
    input.set_gamepads(&[pad([0.0; 4], 1.0)]);
    input.update();
    assert!(!input.action("brake").down);
    input.set_gamepads(&[pad([0.0; 4], 0.0)]);
    input.update();
    input.set_gamepads(&[pad([0.0; 4], 1.0)]);
    input.update();
    assert!(input.pressed("brake"));
}

#[test]
fn contexts_gate_actions_axes_and_bound_keys() {
    let mut input = manager();
    assert!(input.binds_key(key::SPACE) && !input.binds_key(key::ENTER));
    input.handle_key(key::ENTER, true);
    input.update();
    assert!(!input.action("menu").down);

    input.set_contexts(["menu"]);
    assert!(input.binds_key(key::ENTER) && !input.binds_key(key::SPACE));
    input.handle_key(key::ENTER, true);
    input.handle_key(key::KEY_W, true);
    input.update();
    assert!(input.pressed("menu"));
    assert!(!input.action("forward").down);
    assert_eq!(input.axis("throttle"), 0.0);

    input.set_contexts([]);
    input.update();
    assert!(!input.action("menu").down);
}

#[test]
fn rebind_replaces_bindings_and_rejects_unknown_actions() {
    let mut input = manager();
    input.handle_key(key::ESCAPE, true);
    input.rebind("pause", [InputBinding::key(key::KEY_P)]).unwrap();
    input.update();
    assert!(!input.action("pause").down, "rebinding releases holds");
    input.handle_key(key::KEY_P, true);
    input.update();
    assert!(input.pressed("pause"));
    assert_eq!(input.rebind("missing", []), Err(InputError::UnknownAction("missing".to_owned())));
}

#[test]
fn suspension_ignores_presses_but_applies_releases() {
    let mut input = manager();
    input.set_focused(false);
    input.handle_key(key::KEY_W, true);
    input.set_virtual("forward", 1.0);
    input.set_gamepads(&[pad([1.0, 0.0, 0.0, 0.0], 1.0)]);
    input.update();
    assert!(!input.action("forward").down && !input.action("brake").down);
    assert_eq!(input.axis("steer"), 0.0);

    input.set_focused(true);
    input.handle_key(key::KEY_W, true);
    input.update();
    assert!(input.action("forward").down);
    input.set_typing(true);
    input.update();
    assert!(input.action("forward").released, "starting to type releases holds");
    input.set_typing(false);
    input.set_visible(false);
    input.handle_key(key::KEY_W, true);
    input.update();
    assert!(!input.action("forward").down);
}

#[test]
fn pointer_motion_accumulates_per_frame_and_stops_while_suspended() {
    let mut input = manager();
    input.handle_pointer_motion(Vec2::new(3.0, -1.0));
    input.handle_pointer_motion(Vec2::new(2.0, 4.0));
    assert_eq!(input.pointer_motion(), Vec2::ZERO, "motion reads after the next update");
    input.update();
    assert_eq!(input.pointer_motion(), Vec2::new(5.0, 3.0));
    input.update();
    assert_eq!(input.pointer_motion(), Vec2::ZERO, "one frame consumes its motion");

    input.handle_pointer_motion(Vec2::new(1.0, 1.0));
    input.clear();
    input.update();
    assert_eq!(input.pointer_motion(), Vec2::ZERO, "clearing drops pending motion");
    input.set_focused(false);
    input.handle_pointer_motion(Vec2::new(1.0, 1.0));
    input.update();
    assert_eq!(input.pointer_motion(), Vec2::ZERO, "suspended input ignores motion");
    input.set_focused(true);
    input.handle_pointer_motion(Vec2::new(f32::NAN, 1.0));
    input.update();
    assert_eq!(input.pointer_motion(), Vec2::ZERO, "non-finite motion is rejected");

    assert!(!input.pointer_locked());
    input.set_pointer_locked(true);
    assert!(input.pointer_locked());
}

#[test]
fn winit_key_names_match_dom_codes() {
    use winit::keyboard::KeyCode;
    assert_eq!(key_code_name(KeyCode::KeyW), key::KEY_W);
    assert_eq!(key_code_name(KeyCode::ArrowUp), key::ARROW_UP);
    assert_eq!(key_code_name(KeyCode::Space), key::SPACE);
    assert_eq!(key_code_name(KeyCode::Escape), key::ESCAPE);
    assert_eq!(key_code_name(KeyCode::Backquote), key::BACKQUOTE);
    assert_eq!(key_code_name(KeyCode::Digit1), key::DIGIT1);
    assert_eq!(key_code_name(KeyCode::ShiftLeft), key::SHIFT_LEFT);
}
