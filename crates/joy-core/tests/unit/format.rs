// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/format.rs`.

use super::*;

#[test]
fn numbers_print_in_shortest_round_trip_form() {
    assert_eq!(format_number(1.0), "1");
    assert_eq!(format_number(-0.0), "0");
    assert_eq!(format_number(0.000001), "0.000001");
    assert_eq!(format_number(1.5e-7), "1.5e-7");
    assert_eq!(format_number(1e21), "1e+21");
    assert_eq!(format_number(std::f64::consts::FRAC_PI_2), "1.5707963267948966");
    assert_eq!(format_number(f64::NAN), "NaN");
    assert_eq!(json_number(2.0).to_string(), "2");
    assert_eq!(json_number(0.25).to_string(), "0.25");
    assert_eq!(json_number(f64::INFINITY), Value::Null);
}
