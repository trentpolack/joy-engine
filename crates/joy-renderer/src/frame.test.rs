// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/frame.rs`.

use super::*;

#[test]
fn ambient_default_depends_on_environment() {
    assert_eq!(FrameInputs::default().resolved_ambient(), DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT);
}
