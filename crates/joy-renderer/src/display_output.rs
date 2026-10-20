// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The presentation target's signal: how the post-processing chain encodes what it writes, and how far above standard
//! (SDR) white the display can show highlights right now.
//!
//! On a standard display, `1.0` is the brightest value: the white of a UI panel. HDR displays (Apple XDR panels, HDR
//! monitors and TVs) can show highlights several times brighter than that white, but only to apps whose surface asks for
//! an extended-range signal. Their *headroom* is the multiple of SDR white they can show at this moment; on Apple
//! displays it rises as the brightness slider falls. The renderer keeps SDR white at `1.0` in every mode, tone maps
//! highlights into the headroom, and leaves normally lit content and the UI exactly as bright as on a standard display.
//!
//! Extended linear (scRGB) signals add one wrinkle: Windows defines their `1.0` as 80 nits rather than as SDR white, and
//! shows SDR content at a brighter, user-chosen level. [`DisplayOutput::sdr_white_level`] carries that level so the
//! presentation pass and the UI scale their light to match the rest of the desktop.

/// Largest headroom the renderer uses, a multiple of SDR white; brighter claims are clamped. Also bounds the SDR white
/// level.
pub const MAX_DISPLAY_HEADROOM: f32 = 16.0;

/// How the presentation target interprets the values the renderer writes to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputEncoding {
    /// Standard dynamic range: sRGB-encoded values from `0` to `1`. An sRGB-format view encodes them on write.
    #[default]
    Standard,
    /// Extended-range linear light (scRGB): `1.0` is SDR white and highlights go above it. Native HDR surfaces that
    /// expect a linear signal (`wgpu::SurfaceColorSpace::ExtendedSrgbLinear`, the only HDR form DirectX 12 offers).
    ExtendedLinear,
    /// Extended-range sRGB: the sRGB curve continued above `1.0` (`wgpu::SurfaceColorSpace::ExtendedSrgb`). Browser HDR
    /// canvases and native surfaces that offer it.
    ExtendedSrgb,
}

impl OutputEncoding {
    /// Whether the signal can carry values above SDR white.
    pub fn is_extended(self) -> bool {
        self != Self::Standard
    }
}

/// The presentation target's encoding, the display's current HDR headroom, and the signal level of SDR white.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayOutput {
    /// What the presentation target expects.
    pub encoding: OutputEncoding,
    /// Multiple of SDR white the display can show right now; `1.0` for standard dynamic range.
    pub headroom: f32,
    /// Linear signal value of SDR white for [`OutputEncoding::ExtendedLinear`] output: `1.0` where the platform defines
    /// scRGB `1.0` as SDR white (Apple displays), and the SDR content brightness over 80 nits on Windows. Other
    /// encodings ignore it.
    pub sdr_white_level: f32,
}

impl DisplayOutput {
    /// Standard dynamic range: sRGB-encoded values with no headroom.
    pub const STANDARD: Self = Self {
        encoding: OutputEncoding::Standard,
        headroom: 1.0,
        sdr_white_level: 1.0,
    };

    /// An extended-range output with `headroom` and `sdr_white_level`, each clamped to `1..=MAX_DISPLAY_HEADROOM`
    /// (non-finite values mean `1.0`).
    pub fn extended(encoding: OutputEncoding, headroom: f32, sdr_white_level: f32) -> Self {
        Self {
            encoding,
            headroom: sanitized_multiple(headroom),
            sdr_white_level: sanitized_multiple(sdr_white_level),
        }
    }

    /// Whether highlights can go above SDR white: an extended signal and a display with headroom.
    pub fn is_high_dynamic_range(&self) -> bool {
        self.encoding.is_extended() && self.headroom > 1.0
    }

    /// The headroom the post-processing chain tone maps into: `1.0` for a standard signal, else the clamped headroom.
    pub fn effective_headroom(&self) -> f32 {
        if self.encoding.is_extended() { sanitized_multiple(self.headroom) } else { 1.0 }
    }

    /// The linear signal value the presentation pass and the UI write for SDR white: the clamped
    /// [`DisplayOutput::sdr_white_level`] for extended linear output, else `1.0`.
    pub fn effective_sdr_white_level(&self) -> f32 {
        if self.encoding == OutputEncoding::ExtendedLinear { sanitized_multiple(self.sdr_white_level) } else { 1.0 }
    }
}

impl Default for DisplayOutput {
    fn default() -> Self {
        Self::STANDARD
    }
}

impl std::fmt::Display for DisplayOutput {
    /// `SDR`, or `HDR 3.2x` with the current headroom.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.encoding.is_extended() {
            write!(formatter, "HDR {:.1}x", self.effective_headroom())
        } else {
            formatter.write_str("SDR")
        }
    }
}

/// Clamp a reported multiple of SDR white to the range the renderer uses; unknown (non-finite) values mean `1.0`.
fn sanitized_multiple(multiple: f32) -> f32 {
    if multiple.is_finite() { multiple.clamp(1.0, MAX_DISPLAY_HEADROOM) } else { 1.0 }
}
