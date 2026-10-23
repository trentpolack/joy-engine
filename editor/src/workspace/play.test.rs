// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn play_requires_a_dev_script_and_starts_idle() {
    assert!(defines_dev_script(r#"{"scripts": {"dev": "cargo run -p luna"}}"#));
    assert!(!defines_dev_script(r#"{"scripts": {"build": "x"}}"#));
    assert!(!defines_dev_script(r#"{"scripts": {"dev": "  "}}"#));
    assert!(!defines_dev_script("not json"));

    let mut session = PlaySession::new();
    assert_eq!(session.state(), PlayState::Idle);
    let missing = std::env::temp_dir().join("joy-editor-play-missing-project");
    assert!(session.start("Missing", &missing).unwrap_err().contains("package.json"));
    assert_eq!(session.state(), PlayState::Idle);
    assert!(session.poll().is_none());
}
