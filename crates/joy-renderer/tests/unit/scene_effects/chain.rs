// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene_effects/chain.rs`.

use super::*;

#[test]
fn texture_scale_corrects_aspect_like_luma() {
    assert_eq!(texture_scale((1920, 1080), (960, 540)), [1.0, 1.0]);
    assert_eq!(texture_scale((100, 100), (200, 100)), [1.0, 2.0]);
    assert_eq!(texture_scale((200, 100), (100, 100)), [2.0, 1.0]);
}
