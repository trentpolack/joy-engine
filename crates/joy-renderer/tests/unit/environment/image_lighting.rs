// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/environment/image_lighting.rs`.

use super::*;

#[test]
fn half_floats_match_ieee_encoding() {
    assert_eq!(encode_positive_half(0.0), 0);
    assert_eq!(encode_positive_half(1.0), 0x3c00);
    assert_eq!(encode_positive_half(0.5), 0x3800);
    assert_eq!(encode_positive_half(65504.0), 0x7bff);
    assert_eq!(encode_positive_half(2_f32.powi(-24)), 1);
    // Rounding up the mantissa carries into the exponent.
    assert_eq!(encode_positive_half(1.9999), 0x4000);
}
