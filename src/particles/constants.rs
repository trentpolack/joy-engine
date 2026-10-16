// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Serialized particle vocabularies shared by `.joyfx` documents, the script language, and renderers.
//!
//! Spellings match the TypeScript `src/particles/constants.ts` so the same documents load in both engines.

/// Particle renderer families (the emitter `renderer` field).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleRenderer {
    /// Feathered radial disc.
    Soft,
    /// World-space velocity ribbon with a soft glow.
    Streak,
    /// Thin annulus.
    Ring,
    /// Overlapping feathered lobes for smoke and fire volume.
    Billow,
    /// Solid disc.
    Circle,
    /// Rotated solid quad.
    Square,
    /// Rotated quad sampling one frame of an embedded PNG.
    Textured,
    /// Rotated quad stepping through an embedded PNG atlas over time.
    Flipbook,
}

impl ParticleRenderer {
    /// Every renderer, in the TypeScript declaration order.
    pub const ALL: [Self; 8] = [
        Self::Soft,
        Self::Streak,
        Self::Ring,
        Self::Billow,
        Self::Circle,
        Self::Square,
        Self::Textured,
        Self::Flipbook,
    ];

    /// The serialized spelling (`"soft"`, `"flipbook"`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Streak => "streak",
            Self::Ring => "ring",
            Self::Billow => "billow",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Textured => "textured",
            Self::Flipbook => "flipbook",
        }
    }

    /// Parse a serialized spelling; matching is exact and case-sensitive.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|renderer| renderer.as_str() == text)
    }

    /// Whether the renderer samples an embedded texture (`textured` and `flipbook`).
    pub fn is_textured(self) -> bool {
        matches!(self, Self::Textured | Self::Flipbook)
    }
}

/// Serialized effect parameter types (the parameter `type` field; omitted means scalar).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleParameterType {
    /// One number.
    Scalar,
    /// Three numbers exposed to scripts as `nameX`, `nameY`, `nameZ` and the vector `name`.
    Vector3,
    /// Four numbers in `[0, 1]` exposed to scripts as the color `name`.
    Color,
}

impl ParticleParameterType {
    /// The serialized spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Vector3 => "vector3",
            Self::Color => "color",
        }
    }
}

/// Particle script value categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleValueKind {
    /// One number.
    Scalar,
    /// XYZ components.
    Vector,
    /// RGBA components.
    Color,
}

impl ParticleValueKind {
    /// The spelling used in script diagnostics (`"scalar"`, `"vector"`, `"color"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Vector => "vector",
            Self::Color => "color",
        }
    }

    /// Component letters accepted after a dot (`x y z` or `r g b a`); scalars have none.
    pub fn component_names(self) -> &'static [&'static str] {
        match self {
            Self::Scalar => &[],
            Self::Vector => &["x", "y", "z"],
            Self::Color => &["r", "g", "b", "a"],
        }
    }

    /// Instruction-budget cost of evaluating one value of this kind (one per component).
    pub(crate) fn cost(self) -> i64 {
        match self {
            Self::Scalar => 1,
            Self::Vector => 3,
            Self::Color => 4,
        }
    }
}

impl std::fmt::Display for ParticleValueKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_spellings_round_trip() {
        for renderer in ParticleRenderer::ALL {
            assert_eq!(ParticleRenderer::parse(renderer.as_str()), Some(renderer));
        }
        assert_eq!(ParticleRenderer::parse("Soft"), None);
        assert!(ParticleRenderer::Flipbook.is_textured() && !ParticleRenderer::Billow.is_textured());
    }
}
