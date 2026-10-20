// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/console_variables.rs`.

use super::*;

fn registry() -> ConsoleVariables {
    let mut variables = ConsoleVariables::new();
    variables
        .register(VariableDefinition::float("r.ScreenPercentage", "Scene resolution.", 100.0, 25.0, 100.0))
        .unwrap();
    variables.register(VariableDefinition::bool("r.TAA", "Temporal antialiasing.", false)).unwrap();
    variables
        .register(VariableDefinition::choice("r.DebugView", "Surface view.", &["lit", "baseColor"], "lit"))
        .unwrap();
    variables.register(VariableDefinition::integer("game.Lives", "Starting lives.", 3, 1, 9)).unwrap();
    variables
}

#[test]
fn values_parse_by_kind_and_range_with_names_ignoring_case() {
    let mut variables = registry();
    assert_eq!(variables.set("r.screenpercentage", "75").unwrap(), VariableValue::Float(75.0));
    assert_eq!(variables.float("R.ScreenPercentage"), Some(75.0));
    assert_eq!(variables.set("r.TAA", "on").unwrap(), VariableValue::Bool(true));
    assert_eq!(variables.set("r.DebugView", "BASECOLOR").unwrap(), VariableValue::Text("baseColor".into()));

    // Out of range, the wrong kind, and unknown names are refused and leave values alone.
    let refused = variables.set("r.ScreenPercentage", "150").unwrap_err();
    assert_eq!(refused.to_string(), "r.ScreenPercentage accepts 25 to 100.");
    assert!(variables.set("game.Lives", "2.5").is_err());
    assert!(variables.set("r.DebugView", "wireframe").is_err());
    assert_eq!(variables.set("r.Missing", "1"), Err(ConsoleError::Unknown("r.Missing".into())));
    assert_eq!(variables.float("r.ScreenPercentage"), Some(75.0));

    // Registration refuses duplicates (in any case), bad names, and defaults that do not fit.
    let duplicate = VariableDefinition::bool("R.TAA", "Again.", true);
    assert!(matches!(variables.register(duplicate), Err(ConsoleError::Duplicate(_))));
    assert!(matches!(
        variables.register(VariableDefinition::bool("2fast", "", true)),
        Err(ConsoleError::InvalidName(_))
    ));
    assert!(variables.register(VariableDefinition::float("r.Bad", "", 5.0, 0.0, 1.0)).is_err());
}

#[test]
fn changes_queue_once_per_owner_prefix_and_mirroring_never_queues() {
    let mut variables = registry();
    variables.set("r.TAA", "1").unwrap();
    variables.set("game.Lives", "5").unwrap();
    variables.set("r.ScreenPercentage", "50").unwrap();
    variables.set("r.TAA", "1").unwrap();
    assert_eq!(variables.take_changes("r."), ["r.TAA", "r.ScreenPercentage"]);
    assert!(variables.take_changes("r.").is_empty());
    assert_eq!(variables.take_changes("game."), ["game.Lives"]);

    // An owner reporting its real state updates the display without asking itself to apply it.
    variables.mirror("r.ScreenPercentage", VariableValue::Float(80.0));
    variables.mirror("r.ScreenPercentage", VariableValue::Float(500.0));
    assert_eq!(variables.float("r.ScreenPercentage"), Some(80.0));
    assert!(variables.take_changes("r.").is_empty());

    // Resetting queues the default.
    assert_eq!(variables.reset("r.ScreenPercentage").unwrap(), VariableValue::Float(100.0));
    assert_eq!(variables.take_changes("r."), ["r.ScreenPercentage"]);
    assert_eq!(variables.complete("r.s"), ["r.ScreenPercentage"]);
}
