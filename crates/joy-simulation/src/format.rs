// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Number formatting shared by error messages, so range errors read the same as the TypeScript engine's.

/// Format a range bound the way JavaScript prints numbers (`Infinity`, `-Infinity`, `5e-324`, `0.001`).
pub(crate) fn format_bound(value: f64) -> String {
    if value == f64::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value != 0.0 && value.abs() < 1e-6 {
        return format!("{value:e}");
    }
    format!("{value}")
}
