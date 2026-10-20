// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/dock.rs`.

use super::*;

fn run(context: &Context, ui: impl FnMut(&Context)) {
    let mut ui = ui;
    let mut output = context.run_ui(egui::RawInput::default(), |root| ui(root.ctx()));
    // egui requires texture updates to be consumed; these tests never paint.
    output.textures_delta.clear();
}

#[test]
fn one_panel_opens_at_a_time_and_escape_closes_it() {
    let context = Context::default();
    run(&context, |context| {
        launcher(context, DockTool::Stats, RichText::new("STATS"));
        launcher(context, DockTool::PostProcessing, RichText::new("JOY-ENGINE"));
    });
    assert_eq!(open_tool(&context), None);
    assert!(reserved_height(&context) > 0.0);
    set_open(&context, DockTool::Stats, true);
    set_open(&context, DockTool::PostProcessing, true);
    assert_eq!(open_tool(&context), Some(DockTool::PostProcessing));
    set_open(&context, DockTool::Stats, false);
    assert_eq!(open_tool(&context), Some(DockTool::PostProcessing), "closing another tool keeps this one");
    assert!(close_open_panel(&context));
    assert!(!close_open_panel(&context));
}

#[test]
fn steps_round_values_to_their_precision() {
    assert_eq!(step_decimals(0.01), 2);
    assert_eq!(step_decimals(0.5), 1);
    assert_eq!(step_decimals(1.0), 0);
    assert_eq!(round_to_step(0.123_456, 0.001), 0.123);
}

#[test]
fn claimed_keys_expire_when_tools_stop_claiming_them() {
    let context = Context::default();
    run(&context, |context| claim_key(context, "KeyP"));
    assert!(key_claimed(&context, "KeyP") && !key_claimed(&context, "KeyQ"));
    run(&context, |_| {});
    run(&context, |_| {});
    assert!(!key_claimed(&context, "KeyP"));
}
