// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Color records and parsing. Colors are linear RGB(A) unless a function documents otherwise.
//!
//! [`Color`] (`f64` channels) matches the TypeScript `Color` record and its helpers bit for bit; [`Rgb`] and [`Rgba`]
//! are the `f32` colors GPU-facing code uploads.

use crate::types::Color;

/// Linear RGB color for GPU-facing records.
pub type Rgb = [f32; 3];

/// Straight-alpha linear RGBA color for GPU-facing records.
pub type Rgba = [f32; 4];

/// Largest 8-bit channel value; hex channels are divided by it.
const CHANNEL_MAXIMUM: f64 = 255.0;

/// Return a new color with a replacement alpha; the source color is copied, never modified.
pub fn with_alpha(color: Color, alpha: f64) -> Color {
    [color[0], color[1], color[2], alpha]
}

/// Decode `#RGB` or `#RRGGBB` notation (case-insensitive) into normalized RGBA with opaque alpha.
///
/// Unsupported forms return `None` so callers choose their own fallback. Channels are the encoded values divided by
/// 255 in `f64`, exactly like the TypeScript `parseHexColor`; no color-space conversion is applied.
pub fn parse_hex_color(value: &str) -> Option<Color> {
    let digits = value.strip_prefix('#')?;
    if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }

    // Expand the short form so both notations share one decoder.
    let expanded: String = match digits.len() {
        3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
        6 => digits.to_owned(),
        _ => return None,
    };

    let channel = |offset: usize| -> Option<f64> {
        let byte = u8::from_str_radix(&expanded[offset..offset + 2], 16).ok()?;
        Some(f64::from(byte) / CHANNEL_MAXIMUM)
    };
    Some([channel(0)?, channel(2)?, channel(4)?, 1.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(parse_hex_color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_hex_color("#FF0000"), Some([1.0, 0.0, 0.0, 1.0]));
        // 0x33 / 255 as a JavaScript number.
        assert_eq!(parse_hex_color("#336699").map(|color| color[0]), Some(0.2));
        assert_eq!(parse_hex_color("#12"), None);
        assert_eq!(parse_hex_color("fff"), None);
        assert_eq!(parse_hex_color("#ggg"), None);
    }

    #[test]
    fn with_alpha_copies_rgb() {
        assert_eq!(with_alpha([0.1, 0.2, 0.3, 1.0], 0.5), [0.1, 0.2, 0.3, 0.5]);
    }
}
