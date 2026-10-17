// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The real interface on a headless device with synthetic input, ported from the TypeScript `live-apply-browser.mjs`
//! and `browser.mjs` checks.

use joy_engine::egui::{
    Key,
    Modifiers,
};
use particle_lab::lab::PreviewState;

use crate::support::Harness;

#[test]
fn the_lab_starts_with_the_first_example_live() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.idle(30);
    harness.capture("particle-lab-start");
    let lab = harness.tool.lab();
    assert!(lab.effect().is_some(), "the first example is applied");
    assert_eq!(lab.asset().name, "Ember Impact");
    assert_eq!(lab.preview_state(), PreviewState::Live);
    assert!(lab.time() > 0.3, "playback runs: {}", lab.time());
    assert!(harness.find("tint A value").is_some(), "parameter fields have accessible names: {:?}", harness.names());
    // Opaque Joy panels surround the open preview, which shows the slate editor background behind the particles.
    let panel = joy_engine::runtime::theme::tokens::PANEL;
    assert!(close(harness.pixel(640, 20), [panel.r(), panel.g(), panel.b()]), "header {:?}", harness.pixel(640, 20));
    let region = harness.tool.interface_mut().region().unwrap();
    assert!(region.size.x > 500.0 && region.size.y > 150.0, "{region:?}");
    let corner = harness.pixel(region.min.x as u32 + 4, region.min.y as u32 + 4);
    assert!(!close(corner, [panel.r(), panel.g(), panel.b()]), "the preview region is open: {corner:?}");
}

/// Whether two colors are within a few 8-bit levels.
fn close(left: [u8; 3], right: [u8; 3]) -> bool {
    left.iter().zip(right).all(|(left, right)| left.abs_diff(right) <= 4)
}

#[test]
fn live_off_cancels_queued_applies_explicit_apply_works_and_invalid_drafts_stay_separate() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.idle(2);
    let alpha = |harness: &Harness| match &harness.tool.lab().asset().parameters["tint"] {
        joy_engine::particles::ParticleParameter::Color { value, .. } => value[3],
        _ => unreachable!("tint is a color"),
    };
    let accepted_alpha = |harness: &Harness| match &harness
        .tool
        .lab()
        .definition()
        .expect("an accepted effect")
        .asset()
        .parameters["tint"]
    {
        joy_engine::particles::ParticleParameter::Color { value, .. } => value[3],
        _ => unreachable!("tint is a color"),
    };

    // An edit is pending; turning Live off cancels the queued automatic apply.
    harness.fill("tint A value", "0.8");
    assert_eq!(alpha(&harness), 0.8);
    assert_eq!(harness.tool.lab().preview_state(), PreviewState::Pending);
    harness.click_named("Live");
    assert!(!harness.tool.lab().live());
    harness.fill("tint A value", "0.7");
    harness.idle(70);
    assert_eq!(harness.tool.lab().preview_state(), PreviewState::Pending, "Live off never applies automatically");
    assert_eq!(alpha(&harness), 0.7, "Live off retains the edited definition");
    assert_eq!(accepted_alpha(&harness), 1.0, "the accepted preview is unchanged");

    // Explicit Apply accepts the definition with Live off.
    harness.click_named("Apply");
    assert_eq!(harness.tool.lab().preview_state(), PreviewState::Live);
    assert_eq!(accepted_alpha(&harness), 0.7);

    // An out-of-range draft keeps its error, never replaces values, and blocks Apply.
    harness.fill("tint A value", "2");
    assert!(harness.tool.lab().drafts.has_rejected());
    assert_eq!(alpha(&harness), 0.7);
    harness.click_named("Apply");
    assert_eq!(harness.tool.lab().preview_state(), PreviewState::Live, "LIVE still describes the accepted definition");
    assert_eq!(accepted_alpha(&harness), 0.7);
    assert!(harness.tool.lab_mut().export_text().is_err(), "rejected drafts are not exported");
    harness.fill("tint A value", "0.6");
    assert!(!harness.tool.lab().drafts.has_rejected(), "the repaired draft clears");
    assert_eq!(harness.tool.lab().preview_state(), PreviewState::Pending);
    harness.capture("particle-lab-live-apply");

    // Turning Live back on applies the pending edit after the delay.
    harness.click_named("Live");
    harness.idle(30);
    assert_eq!(harness.tool.lab().preview_state(), PreviewState::Live);
    assert_eq!(accepted_alpha(&harness), 0.6);
}

#[test]
fn script_edits_show_diagnostics_and_ctrl_enter_applies() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.click_named("SPAWN SCRIPT");
    harness.idle(1);
    // Click inside the editor, select everything, and type an unfinished expression.
    let region = harness.tool.interface_mut().region().unwrap();
    harness.click(joy_engine::egui::pos2(region.min.x - 160.0, 560.0));
    harness.key(Key::A, Modifiers::COMMAND);
    harness.type_text("size = (");
    harness.idle(1);
    assert_eq!(harness.tool.lab().asset().emitters[0].spawn, "size = (");
    assert_eq!(harness.tool.lab().script_diagnostics().0.len(), 1);
    let accepted = harness.tool.lab().definition().unwrap().clone();
    harness.key(Key::Enter, Modifiers::COMMAND);
    harness.idle(2);
    assert!(harness.tool.lab().error().is_some(), "Ctrl+Enter applied and reported the script error");
    assert!(harness.tool.lab().definition().unwrap().ptr_eq(&accepted));
    harness.capture("particle-lab-script-error");

    // Completion inserts a call with the cursor inside.
    harness.key(Key::A, Modifiers::COMMAND);
    harness.type_text("size = smo");
    harness.key(Key::Enter, Modifiers::NONE);
    harness.type_text("0, 1, 0.5");
    harness.key(Key::End, Modifiers::NONE);
    harness.type_text(" * 4;");
    harness.idle(1);
    assert_eq!(harness.tool.lab().asset().emitters[0].spawn, "size = smoothstep(0, 1, 0.5) * 4;");
    harness.key(Key::Enter, Modifiers::COMMAND);
    harness.idle(2);
    assert!(harness.tool.lab().error().is_none(), "{:?}", harness.tool.lab().error());
}

#[test]
fn keyboard_shortcuts_and_camera_gestures_drive_the_preview() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let region = harness.tool.interface_mut().region().unwrap();
    let center = joy_engine::egui::pos2(region.min.x + (region.size.x * 0.5), region.min.y + (region.size.y * 0.5));
    harness.click(center);
    assert!(harness.tool.lab().playing());
    harness.key(Key::Space, Modifiers::NONE);
    assert!(!harness.tool.lab().playing(), "Space pauses outside text fields");
    harness.key(Key::R, Modifiers::NONE);
    assert!(harness.tool.lab().playing(), "R restarts");

    let yaw = harness.tool.lab().viewport.camera().yaw;
    harness.frame(vec![crate::support::button(center, true)]);
    for step in 1..=5 {
        harness.frame(vec![joy_engine::egui::Event::PointerMoved(
            center + joy_engine::egui::vec2(12.0 * step as f32, 0.0),
        )]);
    }
    harness.frame(vec![crate::support::button(center + joy_engine::egui::vec2(60.0, 0.0), false)]);
    assert!(harness.tool.lab().viewport.camera().yaw < yaw - 0.2, "dragging orbits");
    let distance = harness.tool.lab().viewport.camera().distance;
    harness.frame(vec![joy_engine::egui::Event::MouseWheel {
        unit: joy_engine::egui::MouseWheelUnit::Point,
        delta: joy_engine::egui::vec2(0.0, 100.0),
        phase: joy_engine::egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    }]);
    assert!(harness.tool.lab().viewport.camera().distance < distance, "scrolling away zooms in");
    harness.key(Key::Num0, Modifiers::NONE);
    assert_eq!(harness.tool.lab().viewport.camera().yaw, 0.75, "0 resets the orbit");
}

#[test]
fn a_slider_drag_is_one_property_undo_and_emitters_are_added_and_selected() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let slider = harness.find("power slider").expect("the power slider");
    harness.frame(vec![joy_engine::egui::Event::PointerMoved(slider)]);
    harness.frame(vec![crate::support::button(slider, true)]);
    for step in 1..=4 {
        harness.frame(vec![joy_engine::egui::Event::PointerMoved(
            slider + joy_engine::egui::vec2(10.0 * step as f32, 0.0),
        )]);
    }
    harness.frame(vec![crate::support::button(slider + joy_engine::egui::vec2(40.0, 0.0), false)]);
    let power = |harness: &Harness| match &harness.tool.lab().asset().parameters["power"] {
        joy_engine::particles::ParticleParameter::Scalar { value, .. } => *value,
        _ => unreachable!(),
    };
    assert!(power(&harness) > 1.2, "{}", power(&harness));
    harness.click_named("Undo property");
    assert_eq!(power(&harness), 1.0, "the whole drag undoes at once");
    assert!(!harness.tool.lab().can_undo());

    harness.click_named("Add emitter");
    harness.idle(1);
    harness.click_named("Ring Flash\nExpanding rings for an impact.");
    harness.idle(1);
    assert_eq!(harness.tool.lab().asset().emitters.len(), 5);
    assert_eq!(harness.tool.lab().current_emitter(), 4);
    harness.click_named("sparks");
    assert_eq!(harness.tool.lab().current_emitter(), 1);
    harness.capture("particle-lab-emitters");
}

#[test]
fn hosted_tabs_hide_their_file_chrome() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    use joy_engine::development::{
        AuthoringTool,
        ToolHosting,
    };
    let area =
        joy_engine::egui::Rect::from_min_size(joy_engine::egui::pos2(0.0, 40.0), joy_engine::egui::vec2(1280.0, 760.0));
    harness.tool.set_hosting(ToolHosting::Hosted { area });
    harness.idle(4);
    assert!(harness.find("Open").is_none() && harness.find("Save .joyfx ↗").is_none(), "{:?}", harness.names());
    assert!(harness.find("Apply").is_some());
    harness.capture("particle-lab-hosted");
}
