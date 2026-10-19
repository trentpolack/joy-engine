// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/constants.rs`.

use super::*;

#[test]
fn renderer_spellings_round_trip() {
    for renderer in ParticleRenderer::ALL {
        assert_eq!(ParticleRenderer::parse(renderer.as_str()), Some(renderer));
    }
    assert_eq!(ParticleRenderer::parse("Soft"), None);
    assert!(ParticleRenderer::Flipbook.is_textured() && !ParticleRenderer::Billow.is_textured());
}
