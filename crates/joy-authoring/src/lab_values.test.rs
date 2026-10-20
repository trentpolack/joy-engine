// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/lab_values.rs`.

use super::*;

#[test]
fn numbers_format_like_javascript_and_round_trip_exactly() {
    let cases = [
        (1.0, "1"),
        (-2.5, "-2.5"),
        (0.1 + 0.2, "0.30000000000000004"),
        (1e21, "1e+21"),
        (123_456_789_012_345_680_000.0, "123456789012345680000"),
        (1e-7, "1e-7"),
        (0.000_001, "0.000001"),
        (-1.5e-9, "-1.5e-9"),
        (-0.0, "0"),
        (145.0, "145"),
        (2.2, "2.2"),
    ];
    for (value, text) in cases {
        assert_eq!(format_number(value), text, "{value:e}");
        assert_eq!(parse_number(text), Some(value).map(|value| if value == 0.0 { 0.0 } else { value }));
    }
    for value in [std::f64::consts::PI, 1.0 / 3.0, 6.02214076e23, 4.9e-324, f64::MAX] {
        assert_eq!(parse_number(&format_number(value)), Some(value), "{value:e}");
    }
}

#[test]
fn drafts_accept_only_finite_numbers_within_rules() {
    let rules = NumericRules {
        minimum: Some(0.0),
        maximum: Some(2.0),
        integer: false,
    };
    assert_eq!(validate_draft(" 1.25 ", &rules), Ok(1.25));
    assert_eq!(validate_draft(".5", &rules), Ok(0.5));
    for text in ["", "abc", "1..2", "Infinity", "inf", "NaN", "1e999", "0x10"] {
        assert_eq!(validate_draft(text, &rules), Err(NOT_A_NUMBER_MESSAGE.to_owned()), "{text}");
    }
    assert_eq!(validate_draft("-1", &rules), Err("Value must be greater than or equal to 0.".to_owned()));
    assert_eq!(validate_draft("3", &rules), Err("Value must be less than or equal to 2.".to_owned()));
    let integer = NumericRules {
        integer: true,
        ..NumericRules::default()
    };
    assert_eq!(validate_draft("2.5", &integer), Err("Enter a whole number.".to_owned()));
    assert_eq!(validate_draft("-4", &integer), Ok(-4.0));
}

#[test]
fn arrow_steps_scale_clamp_and_drop_binary_noise() {
    let rules = NumericRules::bounded(0.0, 2.0);
    assert_eq!(stepped_value(1.0, 0.1, StepScale::Normal, 1.0, &rules), 1.1);
    assert_eq!(stepped_value(1.0, 0.1, StepScale::Fine, 1.0, &rules), 1.01);
    assert_eq!(stepped_value(1.0, 0.1, StepScale::Coarse, -1.0, &rules), 0.0, "coarse steps clamp");
    let mut value = 0.0;
    for _ in 0..3 {
        value = stepped_value(value, 0.1, StepScale::Normal, 1.0, &NumericRules::default());
    }
    assert_eq!(value, 0.3, "0.1 + 0.1 + 0.1 without the trailing 4");
    let integer = NumericRules {
        integer: true,
        ..NumericRules::default()
    };
    assert_eq!(stepped_value(4.0, 1.0, StepScale::Fine, 1.0, &integer), 5.0, "integer fine steps stay whole");
    assert_eq!(stepped_value(4.0, 0.0, StepScale::Normal, 1.0, &NumericRules::default()), 4.1);
    assert_eq!(scrubbed_value(1.0, 9.0, 0.1, StepScale::Normal, &rules), 1.4);
    assert_eq!(scrubbed_value(1.0, -1.0, 0.1, StepScale::Normal, &rules), 1.0, "less than one step is no change");
}

#[test]
fn picker_choices_convert_rgb_to_linear_and_preserve_alpha() {
    let previous = [4.0, 0.5, 0.25, 0.37];
    let picked = picked_color_hex("#ff8000", &previous, &[0.0; 4], &[1.0; 4]).unwrap();
    assert_eq!(picked[0], 1.0);
    assert!((picked[1] - 0.215_860_500_5).abs() < 1e-9, "{}", picked[1]);
    assert_eq!(picked[2], 0.0);
    assert_eq!(picked[3], 0.37, "alpha is untouched");
    let bounded = picked_color([1.0, 1.0, 1.0], &previous, &[0.0; 4], &[0.5, 2.0, 2.0, 1.0]);
    assert_eq!(bounded[0], 0.5, "choices clamp to the owner's bounds");
    assert!(picked_color_hex("orange", &previous, &[], &[]).is_err());
    // Display conversion clamps HDR channels without changing them.
    assert_eq!(color_hex(&previous), "#ffbc89");
    assert_eq!(display_color(&[1.0, 0.0, 0.0, 0.5]), egui::Color32::from_rgba_unmultiplied(255, 0, 0, 128));
    for value in [0.0, 0.002, 0.2, 0.5, 1.0] {
        assert!((display_to_linear(linear_to_display(value)) - value).abs() < 1e-12);
    }
}
