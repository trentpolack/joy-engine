// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/launch.rs`.

use super::*;

#[test]
fn native_arguments_supply_named_values_and_flags() {
    let arguments = || ["--capture", "--tick=48", "--faction=nature", "positional"].map(String::from);
    assert_eq!(argument_parameter(arguments(), "capture").as_deref(), Some(""));
    assert_eq!(argument_parameter(arguments(), "tick").as_deref(), Some("48"));
    assert_eq!(argument_parameter(arguments(), "faction").as_deref(), Some("nature"));
    assert_eq!(argument_parameter(arguments(), "burst"), None);
    assert_eq!(argument_parameter(arguments(), "tic"), None);
}
