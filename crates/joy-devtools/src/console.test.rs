// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/console.rs`.

use super::*;

fn last(console: &Console) -> ConsoleLine {
    console.output().last().cloned().expect("output")
}

#[test]
fn commands_print_set_reset_and_refuse_variables() {
    let mut console = Console::new();
    console.execute("r.screenpercentage 75");
    assert_eq!(
        last(&console),
        ConsoleLine {
            kind: LineKind::Info,
            text: "r.ScreenPercentage = 75".into()
        }
    );
    assert_eq!(console.variables().float(render_variables::SCREEN_PERCENTAGE), Some(75.0));

    console.execute("r.ScreenPercentage");
    assert!(last(&console).text.starts_with("r.ScreenPercentage = 75  [25 to 100]"));
    console.execute("r.ScreenPercentage 400");
    assert_eq!(last(&console).kind, LineKind::Error);
    console.execute("reset r.ScreenPercentage");
    assert_eq!(last(&console).text, "r.ScreenPercentage = 100");

    console.execute("warp 9");
    assert_eq!(
        last(&console),
        ConsoleLine {
            kind: LineKind::Error,
            text: "Unknown command or variable 'warp'. Type help.".into()
        }
    );
    console.execute("clear");
    assert_eq!(console.output().count(), 0);
}

#[test]
fn stat_overlays_turn_profiling_on_while_any_shows() {
    let mut console = Console::new();
    console.execute("stat gpu");
    assert!(console.stats().gpu);
    assert_eq!(console.variables().bool(render_variables::PROFILE), Some(true));
    console.execute("stat unit");
    console.execute("stat gpu");
    assert_eq!(
        console.stats(),
        StatOverlays {
            unit: true,
            gpu: false
        }
    );
    assert_eq!(console.variables().bool(render_variables::PROFILE), Some(true));
    console.execute("stat none");
    assert_eq!(console.variables().bool(render_variables::PROFILE), Some(false));
}

#[test]
fn tab_completes_and_arrows_recall_history() {
    let mut console = Console::new();
    console.input = "r.scr".into();
    console.complete();
    assert_eq!(console.input, "r.ScreenPercentage ");

    // Several matches extend to their shared prefix and list themselves.
    console.input = "r.".into();
    console.complete();
    assert_eq!(console.input, "r.");
    assert!(last(&console).text.contains("r.Volumetrics"));

    console.execute("r.TAA 1");
    console.execute("r.MotionBlur 1");
    console.recall(true);
    assert_eq!(console.input, "r.MotionBlur 1");
    console.recall(true);
    console.recall(true);
    assert_eq!(console.input, "r.TAA 1", "recall stops at the oldest entry");
    console.recall(false);
    console.recall(false);
    assert_eq!(console.input, "", "recalling past the newest clears the line");
}
