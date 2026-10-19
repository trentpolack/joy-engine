// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/color.rs`.

use super::*;

#[test]
fn hex_parses_short_long_and_alpha_forms() {
    assert_eq!(Color::from_hex("#fff"), Some(Color::WHITE));
    assert_eq!(Color::from_hex("#FF000080").map(Color::to_rgba8), Some([255, 0, 0, 128]));
    assert_eq!(Color::from_hex("#336699").map(|color| color.red), Some(0.2));
    assert_eq!(Color::from_rgb_hex(0xED6C39).to_rgba8(), [0xED, 0x6C, 0x39, 0xFF]);
    assert_eq!(Color::from_hex("#12"), None);
    assert_eq!(Color::from_hex("fff"), None);
    assert_eq!(Color::from_hex("#ggg"), None);
}

#[test]
fn transfer_functions_round_trip_including_hdr() {
    // sRGB 0.5 is about 21.4% linear light.
    assert!((srgb_to_linear(0.5) - 0.214_041_140_482_232_55).abs() < 1e-15);
    for value in [0.0, 0.002, 0.04, 0.5, 1.0, 4.0, -0.25] {
        assert!((srgb_to_linear(linear_to_srgb(value)) - value).abs() < 1e-12, "{value}");
    }
    let hdr = LinearColor::rgb(3.0, 0.5, 0.0);
    let round_trip = hdr.to_srgb().to_linear();
    assert!((round_trip.red - 3.0).abs() < 1e-5 && round_trip.alpha == 1.0);
}

#[test]
fn colors_serialize_as_short_arrays_and_read_rgb() {
    let color = LinearColor::new(0.66, 1.5, 0.0, 0.25);
    let value = serde_json::to_value(color).unwrap();
    assert_eq!(value.to_string(), "[0.66,1.5,0.0,0.25]");
    assert_eq!(serde_json::from_str::<LinearColor>("[1, 0.5, 0]").unwrap(), LinearColor::rgb(1.0, 0.5, 0.0));
    assert!(serde_json::from_str::<Color>("[1, 0.5]").is_err());
    assert!(serde_json::from_str::<Color>("[1, 0.5, 0, 1, 2]").is_err());
}
