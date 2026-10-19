// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The engine's two color types and the sRGB transfer functions between them.
//!
//! - [`Color`] is sRGB-encoded with straight alpha: what hex codes, color pickers, palettes, and interface colors
//!   mean. Channels are nominally in `[0, 1]`.
//! - [`LinearColor`] is linear light with straight alpha: what the renderer lights, blends, and tone maps (vertex,
//!   clear, light, tint, fog, and emissive colors). RGB may exceed one for HDR values; alpha is opacity in `[0, 1]`,
//!   and records with no opacity (lights and tints) ignore it.
//!
//! Both store `f32` channels, the precision every GPU consumer uses; colors are cosmetic and never feed simulation.
//! Construction never clamps. Only conversions to 8-bit values clamp, since that is where the `[0, 1]` range is real.
//! Both serialize as `[red, green, blue, alpha]` arrays and read three- or four-element arrays (alpha defaults to one).

use std::fmt;

use serde::de::{
    self,
    SeqAccess,
    Visitor,
};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
};

use crate::math::deterministic;

/// Largest 8-bit channel value.
const CHANNEL_MAXIMUM: f32 = 255.0;

/// Encoded values at or below this decode along the curve's linear segment.
const SRGB_DECODE_THRESHOLD: f64 = 0.04045;

/// Linear values at or below this encode along the curve's linear segment.
const SRGB_ENCODE_THRESHOLD: f64 = 0.0031308;

/// Slope of the curve's linear segment.
const SRGB_LINEAR_SLOPE: f64 = 12.92;

/// Exponent of the curve's power segment.
const SRGB_EXPONENT: f64 = 2.4;

/// Offset of the curve's power segment.
const SRGB_OFFSET: f64 = 0.055;

/// Scale of the curve's power segment (`1 + SRGB_OFFSET`).
const SRGB_SCALE: f64 = 1.055;

// ---------------------------------------------------------------------------------------------------------------------
// Transfer functions
// ---------------------------------------------------------------------------------------------------------------------

/// Decode one sRGB-encoded channel to linear light with the exact IEC 61966-2-1 curve.
///
/// Values above one continue the curve and negative values mirror it, so out-of-range channels round-trip through
/// [`linear_to_srgb`].
pub fn srgb_to_linear(encoded: f64) -> f64 {
    let magnitude = encoded.abs();
    let linear = if magnitude <= SRGB_DECODE_THRESHOLD {
        magnitude / SRGB_LINEAR_SLOPE
    } else {
        deterministic::powf((magnitude + SRGB_OFFSET) / SRGB_SCALE, SRGB_EXPONENT)
    };
    linear.copysign(encoded)
}

/// Encode one linear channel as sRGB with the exact IEC 61966-2-1 curve; the inverse of [`srgb_to_linear`].
pub fn linear_to_srgb(linear: f64) -> f64 {
    let magnitude = linear.abs();
    let encoded = if magnitude <= SRGB_ENCODE_THRESHOLD {
        magnitude * SRGB_LINEAR_SLOPE
    } else {
        (SRGB_SCALE * deterministic::powf(magnitude, 1.0 / SRGB_EXPONENT)) - SRGB_OFFSET
    };
    encoded.copysign(linear)
}

// ---------------------------------------------------------------------------------------------------------------------
// Color
// ---------------------------------------------------------------------------------------------------------------------

/// An sRGB-encoded color with straight alpha, as hex codes, color pickers, palettes, and interface colors mean it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    /// Encoded red, nominally in `[0, 1]`.
    pub red: f32,
    /// Encoded green, nominally in `[0, 1]`.
    pub green: f32,
    /// Encoded blue, nominally in `[0, 1]`.
    pub blue: f32,
    /// Opacity in `[0, 1]`; never encoded.
    pub alpha: f32,
}

impl Color {
    /// Opaque white.
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);

    /// Opaque black.
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// A color from encoded channels and straight alpha.
    pub const fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self { red, green, blue, alpha }
    }

    /// An opaque color from encoded channels.
    pub const fn rgb(red: f32, green: f32, blue: f32) -> Self {
        Self::new(red, green, blue, 1.0)
    }

    /// A color from 8-bit channels (`0..=255`), as image files and interface colors store them.
    pub const fn from_rgba8(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self::new(
            red as f32 / CHANNEL_MAXIMUM,
            green as f32 / CHANNEL_MAXIMUM,
            blue as f32 / CHANNEL_MAXIMUM,
            alpha as f32 / CHANNEL_MAXIMUM,
        )
    }

    /// An opaque color from a `0xRRGGBB` value, such as `Color::from_rgb_hex(0xED6C39)`.
    pub const fn from_rgb_hex(value: u32) -> Self {
        Self::from_rgba8((value >> 16) as u8, (value >> 8) as u8, value as u8, u8::MAX)
    }

    /// Parse `#RGB`, `#RRGGBB`, or `#RRGGBBAA` (case-insensitive); anything else returns `None`, so callers choose
    /// their own fallback.
    pub fn from_hex(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
            return None;
        }

        // Expand the short form so every notation shares one decoder.
        let expanded: String = match digits.len() {
            3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
            6 | 8 => digits.to_owned(),
            _ => return None,
        };
        let channel = |index: usize| u8::from_str_radix(&expanded[(index * 2)..((index * 2) + 2)], 16).ok();
        let alpha = if expanded.len() == 8 { channel(3)? } else { u8::MAX };
        Some(Self::from_rgba8(channel(0)?, channel(1)?, channel(2)?, alpha))
    }

    /// This color with a replacement alpha.
    pub const fn with_alpha(self, alpha: f32) -> Self {
        Self { alpha, ..self }
    }

    /// Decode to linear light; alpha is copied unchanged.
    pub fn to_linear(self) -> LinearColor {
        let decode = |channel: f32| srgb_to_linear(f64::from(channel)) as f32;
        LinearColor::new(decode(self.red), decode(self.green), decode(self.blue), self.alpha)
    }

    /// The channels unchanged, relabelled as linear.
    ///
    /// This is not a conversion. The renderer does not yet encode its output as sRGB, so palettes tuned on screen in
    /// encoded values reach it unconverted to keep their current look; call sites use this to make that explicit.
    /// Replace it with [`Color::to_linear`] once the display pass encodes sRGB.
    pub const fn reinterpret_as_linear(self) -> LinearColor {
        LinearColor::new(self.red, self.green, self.blue, self.alpha)
    }

    /// 8-bit channels, clamped to `[0, 1]` and rounded.
    pub fn to_rgba8(self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha].map(quantize)
    }

    /// `[red, green, blue, alpha]`.
    pub const fn to_array(self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// LinearColor
// ---------------------------------------------------------------------------------------------------------------------

/// A linear-light color with straight alpha, as the renderer lights, blends, and tone maps it.
///
/// RGB may exceed one (HDR emission, light radiance, glow tints). Alpha is opacity in `[0, 1]`; records with no
/// opacity, such as lights and tints, ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinearColor {
    /// Linear red; may exceed one.
    pub red: f32,
    /// Linear green; may exceed one.
    pub green: f32,
    /// Linear blue; may exceed one.
    pub blue: f32,
    /// Opacity in `[0, 1]`.
    pub alpha: f32,
}

impl LinearColor {
    /// Opaque white, the identity tint.
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);

    /// Opaque black.
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// A color from linear channels and straight alpha.
    pub const fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self { red, green, blue, alpha }
    }

    /// An opaque color from linear channels.
    pub const fn rgb(red: f32, green: f32, blue: f32) -> Self {
        Self::new(red, green, blue, 1.0)
    }

    /// An opaque color from `[red, green, blue]`.
    pub const fn from_rgb_array(channels: [f32; 3]) -> Self {
        Self::rgb(channels[0], channels[1], channels[2])
    }

    /// A color from `[red, green, blue, alpha]`.
    pub const fn from_array(channels: [f32; 4]) -> Self {
        Self::new(channels[0], channels[1], channels[2], channels[3])
    }

    /// This color with a replacement alpha.
    pub const fn with_alpha(self, alpha: f32) -> Self {
        Self { alpha, ..self }
    }

    /// RGB multiplied by `factor` (for example a light's intensity); alpha is unchanged.
    pub const fn scaled(self, factor: f32) -> Self {
        Self::new(self.red * factor, self.green * factor, self.blue * factor, self.alpha)
    }

    /// Encode as sRGB; HDR channels continue the curve above one, and alpha is copied unchanged.
    pub fn to_srgb(self) -> Color {
        let encode = |channel: f32| linear_to_srgb(f64::from(channel)) as f32;
        Color::new(encode(self.red), encode(self.green), encode(self.blue), self.alpha)
    }

    /// `[red, green, blue, alpha]`, as vertex and uniform records store it.
    pub const fn to_array(self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    /// `[red, green, blue]`, for records with no opacity.
    pub const fn rgb_array(self) -> [f32; 3] {
        [self.red, self.green, self.blue]
    }
}

impl From<[f32; 4]> for LinearColor {
    fn from(channels: [f32; 4]) -> Self {
        Self::from_array(channels)
    }
}

impl From<[f32; 3]> for LinearColor {
    fn from(channels: [f32; 3]) -> Self {
        Self::from_rgb_array(channels)
    }
}

impl From<LinearColor> for [f32; 4] {
    fn from(color: LinearColor) -> Self {
        color.to_array()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------------------------------------------------

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_channels(&self.to_array(), serializer)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [red, green, blue, alpha] = deserializer.deserialize_seq(ChannelsVisitor)?;
        Ok(Self::new(red, green, blue, alpha))
    }
}

impl Serialize for LinearColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_channels(&self.to_array(), serializer)
    }
}

impl<'de> Deserialize<'de> for LinearColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_array(deserializer.deserialize_seq(ChannelsVisitor)?))
    }
}

/// Serde adapter for a [`LinearColor`] field stored as a three-element `[red, green, blue]` array, for records whose
/// documents never carried alpha: `#[serde(with = "joy_core::color::rgb_array")]`.
pub mod rgb_array {
    use serde::{
        Deserializer,
        Serializer,
    };

    use super::{
        ChannelsVisitor,
        LinearColor,
        serialize_channels,
    };

    /// Write `[red, green, blue]`.
    pub fn serialize<S: Serializer>(color: &LinearColor, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_channels(&color.rgb_array(), serializer)
    }

    /// Read `[red, green, blue]` (or a four-element array) as an opaque color.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<LinearColor, D::Error> {
        Ok(LinearColor::from_array(deserializer.deserialize_seq(ChannelsVisitor)?).with_alpha(1.0))
    }
}

/// Write channels as their shortest `f32` decimals (`0.66`, never `0.6600000262260437`), so documents stay stable
/// whether they are written directly or through `serde_json::Value`.
fn serialize_channels<S: Serializer>(channels: &[f32], serializer: S) -> Result<S::Ok, S::Error> {
    let decimals: Vec<f64> =
        channels.iter().map(|channel| channel.to_string().parse().unwrap_or(f64::from(*channel))).collect();
    decimals.serialize(serializer)
}

/// Reads `[red, green, blue]` or `[red, green, blue, alpha]`.
struct ChannelsVisitor;

impl<'de> Visitor<'de> for ChannelsVisitor {
    type Value = [f32; 4];

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an array of three or four color channels")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut channels = [0.0, 0.0, 0.0, 1.0];
        for (index, channel) in channels.iter_mut().enumerate() {
            match sequence.next_element::<f32>()? {
                Some(value) => *channel = value,
                None if index == 3 => break,
                None => return Err(de::Error::invalid_length(index, &self)),
            }
        }
        if sequence.next_element::<de::IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(5, &self));
        }
        Ok(channels)
    }
}

/// One channel as an 8-bit value: clamped to `[0, 1]`, then rounded.
fn quantize(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * CHANNEL_MAXIMUM).round() as u8
}

#[cfg(test)]
#[path = "../tests/unit/color.rs"]
mod tests;
