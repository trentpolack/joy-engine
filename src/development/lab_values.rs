// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! UI-free value rules behind the lab controls: exact number text, numeric draft validation, keyboard and scrub steps,
//! and linear/sRGB color conversion.
//!
//! Authored values are `f64` and are never rounded for display: number text is the shortest text that reads back as
//! the same value (JavaScript `String(number)`), so an exact field shows every authored digit. Colors are authored as
//! linear RGBA; the sRGB conversions here only feed swatches and pickers, and only an explicit picker choice writes
//! converted values back.

/// Multiplier for Shift + arrow (fine) steps; integer fields step by one instead.
pub const FINE_STEP_FACTOR: f64 = 0.1;

/// Multiplier for Alt + arrow (coarse) steps.
pub const COARSE_STEP_FACTOR: f64 = 10.0;

/// Pointer travel, in logical points, that scrubs a number by one (scaled) step.
pub const SCRUB_POINTS_PER_STEP: f32 = 2.0;

/// The message for drafts that are not numbers at all.
pub const NOT_A_NUMBER_MESSAGE: &str = "Enter a finite number.";

/// The message shown when an action needs every numeric draft repaired first.
pub const REPAIR_DRAFTS_MESSAGE: &str = "Correct the highlighted numeric field before continuing.";

/// Bounds and rules a numeric draft must satisfy before it replaces an accepted value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NumericRules {
    /// Smallest accepted value, inclusive.
    pub minimum: Option<f64>,
    /// Largest accepted value, inclusive.
    pub maximum: Option<f64>,
    /// Whether only whole numbers are accepted.
    pub integer: bool,
}

impl NumericRules {
    /// Rules with inclusive bounds.
    pub fn bounded(minimum: f64, maximum: f64) -> Self {
        Self { minimum: Some(minimum), maximum: Some(maximum), integer: false }
    }

    /// Clamp `value` into the bounds (non-finite bounds are ignored).
    pub fn clamp(&self, value: f64) -> f64 {
        let mut value = value;
        if let Some(maximum) = self.maximum.filter(|bound| bound.is_finite()) {
            value = value.min(maximum);
        }
        if let Some(minimum) = self.minimum.filter(|bound| bound.is_finite()) {
            value = value.max(minimum);
        }
        value
    }

    /// Whether the bounds describe a usable slider range (both finite, maximum above minimum).
    pub fn slider_range(&self) -> Option<std::ops::RangeInclusive<f64>> {
        match (self.minimum, self.maximum) {
            (Some(minimum), Some(maximum)) if minimum.is_finite() && maximum.is_finite() && maximum > minimum => {
                Some(minimum..=maximum)
            }
            _ => None,
        }
    }
}

/// Format a number as JavaScript's `String(number)` does: the shortest text that parses back to the same value, in
/// positional notation for exponents from -7 to 20 and in exponent notation (`1e+21`, `5e-7`) beyond. Negative zero
/// formats as `0`.
pub fn format_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".to_owned() } else { "-Infinity".to_owned() };
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    // `{:e}` prints the shortest round-trip digits as `d.ddde<exponent>`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("Rust always prints an exponent with {:e}");
    let exponent: i32 = exponent.parse().expect("Rust prints a decimal exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digit_count = digits.len() as i32;
    // The decimal point sits after `point` digits (ECMAScript Number::toString's n).
    let point = exponent + 1;
    let mut text = String::new();
    if value < 0.0 {
        text.push('-');
    }
    if digit_count <= point && point <= 21 {
        text.push_str(&digits);
        text.extend(std::iter::repeat_n('0', (point - digit_count) as usize));
    } else if 0 < point && point <= 21 {
        text.push_str(&digits[..point as usize]);
        text.push('.');
        text.push_str(&digits[point as usize..]);
    } else if -6 < point && point <= 0 {
        text.push_str("0.");
        text.extend(std::iter::repeat_n('0', (-point) as usize));
        text.push_str(&digits);
    } else {
        text.push_str(&digits[..1]);
        if digit_count > 1 {
            text.push('.');
            text.push_str(&digits[1..]);
        }
        text.push('e');
        text.push(if exponent < 0 { '-' } else { '+' });
        text.push_str(&exponent.abs().to_string());
    }
    text
}

/// Read a numeric draft: surrounding whitespace is ignored and the text must be one decimal number (`12`, `-0.5`,
/// `.25`, `1e-3`). Returns `None` for anything else, including infinities and `NaN`.
pub fn parse_number(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    let plain = trimmed.strip_prefix(['-', '+']).unwrap_or(trimmed);
    // Rust also reads `inf`, `infinity`, and `nan`; HTML number fields do not.
    if plain.is_empty() || !plain.starts_with(|character: char| character.is_ascii_digit() || character == '.') {
        return None;
    }
    trimmed.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Validate a numeric draft against `rules`, returning the accepted value or the message to show beside the field.
pub fn validate_draft(text: &str, rules: &NumericRules) -> Result<f64, String> {
    let value = parse_number(text).ok_or_else(|| NOT_A_NUMBER_MESSAGE.to_owned())?;
    if rules.integer && value.fract() != 0.0 {
        return Err("Enter a whole number.".to_owned());
    }
    if let Some(minimum) = rules.minimum
        && value < minimum
    {
        return Err(format!("Value must be greater than or equal to {}.", format_number(minimum)));
    }
    if let Some(maximum) = rules.maximum
        && value > maximum
    {
        return Err(format!("Value must be less than or equal to {}.", format_number(maximum)));
    }
    Ok(value)
}

/// Keyboard modifiers that scale a numeric step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepScale {
    /// The authored step.
    #[default]
    Normal,
    /// Shift: a tenth of the step (one for integer fields).
    Fine,
    /// Alt: ten steps.
    Coarse,
}

impl StepScale {
    /// The scale for a key press's modifiers; Shift wins over Alt as in the TypeScript controls.
    pub fn from_modifiers(modifiers: egui::Modifiers) -> Self {
        if modifiers.shift {
            Self::Fine
        } else if modifiers.alt {
            Self::Coarse
        } else {
            Self::Normal
        }
    }

    /// The step multiplier for a field.
    pub fn factor(self, integer: bool) -> f64 {
        match self {
            Self::Normal => 1.0,
            Self::Fine if integer => 1.0,
            Self::Fine => FINE_STEP_FACTOR,
            Self::Coarse => COARSE_STEP_FACTOR,
        }
    }
}

/// One arrow-key step from `value`: `direction` is +1 or -1, the step falls back to 0.1 when not positive, the result
/// is clamped to the rules and rounded to 12 significant digits (`Number(value.toPrecision(12))`) so repeated fine
/// steps do not accumulate binary noise. Fine values need not be multiples of the step.
pub fn stepped_value(value: f64, step: f64, scale: StepScale, direction: f64, rules: &NumericRules) -> f64 {
    let step = if step > 0.0 { step } else { 0.1 };
    let next = rules.clamp(value + (direction * step * scale.factor(rules.integer)));
    let next = round_significant(next, 12);
    if rules.integer { next.round() } else { next }
}

/// The value after scrubbing `points` logical points horizontally from `start`: one scaled step per
/// [`SCRUB_POINTS_PER_STEP`], clamped and rounded like [`stepped_value`].
pub fn scrubbed_value(start: f64, points: f32, step: f64, scale: StepScale, rules: &NumericRules) -> f64 {
    let steps = (points / SCRUB_POINTS_PER_STEP).trunc() as f64;
    if steps == 0.0 {
        return start;
    }
    stepped_value(start, step * steps.abs(), scale, steps.signum(), rules)
}

/// Round to `digits` significant decimal digits, as JavaScript's `Number(value.toPrecision(digits))`.
pub fn round_significant(value: f64, digits: usize) -> f64 {
    if !value.is_finite() || value == 0.0 || digits == 0 {
        return value;
    }
    format!("{value:.precision$e}", precision = digits - 1).parse().unwrap_or(value)
}

// ---------------------------------------------------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------------------------------------------------

/// Encode a linear channel for display: clamped to 0–1, then the sRGB transfer function.
pub fn linear_to_display(value: f64) -> f64 {
    let bounded = value.clamp(0.0, 1.0);
    if bounded <= 0.0031308 { bounded * 12.92 } else { (1.055 * bounded.powf(1.0 / 2.4)) - 0.055 }
}

/// Decode a display (sRGB) channel in 0–1 to linear.
pub fn display_to_linear(value: f64) -> f64 {
    if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}

/// The display color of linear RGBA `values` (missing alpha is opaque) for swatches; never written back.
pub fn display_color(values: &[f64]) -> egui::Color32 {
    let channel = |index: usize| (linear_to_display(values.get(index).copied().unwrap_or(0.0)) * 255.0).round() as u8;
    let alpha = (values.get(3).copied().unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(channel(0), channel(1), channel(2), alpha)
}

/// The `#rrggbb` sRGB hex of linear `values`, as the TypeScript native picker shows it.
pub fn color_hex(values: &[f64]) -> String {
    let channel = |index: usize| (linear_to_display(values.get(index).copied().unwrap_or(0.0)) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}

/// Replace RGB with an explicit display-space choice: each channel is decoded to linear and clamped to the owner's
/// existing per-channel bounds; alpha and any further channels are preserved exactly. Opening or refreshing a picker
/// must never call this, so HDR channels and exact drafts survive until the author picks a color.
pub fn picked_color(display: [f64; 3], previous: &[f64], minimums: &[f64], maximums: &[f64]) -> Vec<f64> {
    let mut next = previous.to_vec();
    next.resize(next.len().max(3), 0.0);
    for (channel, value) in display.iter().enumerate() {
        let linear = display_to_linear(value.clamp(0.0, 1.0));
        let minimum = minimums.get(channel).copied().unwrap_or(f64::NEG_INFINITY);
        let maximum = maximums.get(channel).copied().unwrap_or(f64::INFINITY);
        next[channel] = linear.min(maximum).max(minimum);
    }
    next
}

/// [`picked_color`] from `#rrggbb` text (the TypeScript `pickedColor`), quantized to 8 bits per channel like a native
/// color input. Fails for anything but six hexadecimal digits after `#`.
pub fn picked_color_hex(hex: &str, previous: &[f64], minimums: &[f64], maximums: &[f64]) -> Result<Vec<f64>, String> {
    let digits = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if digits.len() != 6 || !digits.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("\"{hex}\" is not a #rrggbb color."));
    }
    let channel = |index: usize| {
        let byte = u8::from_str_radix(&digits[(index * 2)..((index * 2) + 2)], 16).expect("validated hex digits");
        f64::from(byte) / 255.0
    };
    Ok(picked_color([channel(0), channel(1), channel(2)], previous, minimums, maximums))
}

#[cfg(test)]
mod tests {
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
        let rules = NumericRules { minimum: Some(0.0), maximum: Some(2.0), integer: false };
        assert_eq!(validate_draft(" 1.25 ", &rules), Ok(1.25));
        assert_eq!(validate_draft(".5", &rules), Ok(0.5));
        for text in ["", "abc", "1..2", "Infinity", "inf", "NaN", "1e999", "0x10"] {
            assert_eq!(validate_draft(text, &rules), Err(NOT_A_NUMBER_MESSAGE.to_owned()), "{text}");
        }
        assert_eq!(validate_draft("-1", &rules), Err("Value must be greater than or equal to 0.".to_owned()));
        assert_eq!(validate_draft("3", &rules), Err("Value must be less than or equal to 2.".to_owned()));
        let integer = NumericRules { integer: true, ..NumericRules::default() };
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
        let integer = NumericRules { integer: true, ..NumericRules::default() };
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
}
