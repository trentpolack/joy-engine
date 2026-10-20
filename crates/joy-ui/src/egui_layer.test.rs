// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/egui_layer.rs`.

use super::*;

#[test]
fn browser_modifiers_follow_the_platform_shortcut_key_and_reach_egui() {
    let mac = browser_modifiers(false, false, true, true, true);
    assert!(mac.shift && mac.command && mac.mac_cmd && !mac.ctrl);
    let other = browser_modifiers(true, true, false, true, false);
    assert!(other.alt && other.ctrl && other.command && !other.mac_cmd);

    // egui only learns modifier state from `ModifiersChanged` events, so the browser adapter must send them.
    let context = egui::Context::default();
    let frame = |events: Vec<egui::Event>| {
        let mut output = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |_| {},
        );
        output.textures_delta.clear();
    };
    frame(vec![egui::Event::ModifiersChanged(mac)]);
    assert!(context.input(|input| input.modifiers.shift && input.modifiers.command));
    frame(vec![egui::Event::ModifiersChanged(egui::Modifiers::default())]);
    assert!(context.input(|input| !input.modifiers.shift));
}

#[test]
fn presses_are_claimed_only_over_interactable_areas() {
    let context = egui::Context::default();
    let pass = |context: &egui::Context| {
        let mut output = context.run_ui(egui::RawInput::default(), |root| {
            let context = root.ctx();
            egui::Area::new(egui::Id::new("button")).fixed_pos(egui::pos2(10.0, 10.0)).show(context, |ui| {
                let _ = ui.button("RESET");
            });
            egui::Area::new(egui::Id::new("label"))
                .fixed_pos(egui::pos2(200.0, 10.0))
                .interactable(false)
                .show(context, |ui| ui.label("CHUNKS"));
        });
        // egui requires texture updates to be consumed; this test never paints.
        output.textures_delta.clear();
    };
    pass(&context);
    pass(&context);
    assert!(claims_press(&context, egui::pos2(14.0, 14.0)), "a press on the button belongs to the UI");
    assert!(!claims_press(&context, egui::pos2(205.0, 14.0)), "information panels let presses through");
    assert!(!claims_press(&context, egui::pos2(400.0, 300.0)), "the scene gets presses elsewhere");
}
