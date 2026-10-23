// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn invalid_assets_fail_before_they_can_ship_a_broken_ui() {
    let theme: Value = serde_json::from_str(include_str!("../../../assets/ui/theme-default.json")).unwrap();
    let layout: Value = serde_json::from_str(include_str!("../../../assets/ui/layout.json")).unwrap();
    for (pointer, value, field) in [
        ("/editor/panel", Value::from("#abcdef;"), "theme-default.json.editor.panel"),
        ("/fonts/code", Value::from("Missing Font"), "theme-default.json.fonts.code"),
        ("/opacity/game_panel", Value::from(1.5), "theme-default.json.opacity.game_panel"),
    ] {
        let mut invalid = theme.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        let error = generate(&invalid.to_string(), &layout.to_string()).unwrap_err();
        assert!(error.contains(field), "{error}");
    }
    for (pointer, value) in
        [("/space/md", Value::from(-1)), ("/radius/sm", Value::from(1.5)), ("/game_panel/margin", Value::from(128))]
    {
        let mut invalid = layout.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(generate(&theme.to_string(), &invalid.to_string()).is_err(), "accepted {pointer}");
    }
    let mut invalid = theme.clone();
    invalid.as_object_mut().unwrap().remove("fonts");
    assert!(generate(&invalid.to_string(), &layout.to_string()).unwrap_err().contains("theme-default.json.fonts"));
}
