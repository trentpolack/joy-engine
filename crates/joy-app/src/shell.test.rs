// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/shell.rs`.

use super::*;

#[test]
fn touches_that_start_on_the_interface_stay_there_until_released() {
    let mut owners = TouchOwners::default();
    // A drag that starts on a slider: none of it reaches gameplay.
    assert!(!owners.route_to_gameplay(7, TouchPhase::Started, true));
    assert!(!owners.route_to_gameplay(7, TouchPhase::Moved, false), "moving off the widget stays with the UI");
    assert!(!owners.route_to_gameplay(7, TouchPhase::Ended, false));
    // A touch on the scene reaches gameplay throughout, even with the same identifier reused.
    assert!(owners.route_to_gameplay(7, TouchPhase::Started, false));
    assert!(owners.route_to_gameplay(7, TouchPhase::Moved, true), "passing over a widget does not steal it");
    assert!(owners.route_to_gameplay(7, TouchPhase::Cancelled, false));
    // Simultaneous touches are owned independently.
    assert!(!owners.route_to_gameplay(1, TouchPhase::Started, true));
    assert!(owners.route_to_gameplay(2, TouchPhase::Started, false));
    assert!(!owners.route_to_gameplay(1, TouchPhase::Moved, false));
    assert!(owners.route_to_gameplay(2, TouchPhase::Moved, false));
}
