// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Serialized particle vocabularies shared by `.joyfx` documents, the script language, and renderers.
//!
//! Stable spellings preserve compatibility with existing authored documents.

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
    /// Every renderer, in its stable declaration order.
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

/// How an emitter's particles combine with what is behind them (the emitter `blend` field; omitted means
/// [`ParticleBlend::Alpha`]).
///
/// Colors stay linear HDR in every mode; only the compositing changes. Additive particles add light and need no
/// back-to-front order, so the renderer skips sorting them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ParticleBlend {
    /// Straight alpha, source over: RGB is the particle's color and alpha its coverage (the default and the original
    /// look).
    #[default]
    Alpha,
    /// RGB times alpha is added to what is behind; alpha fades the glow but never darkens.
    Additive,
    /// Premultiplied alpha, source over: RGB is already weighted by alpha, so alpha controls how much the particle
    /// occludes and RGB how much light it adds (alpha zero glows additively, alpha one occludes like
    /// [`ParticleBlend::Alpha`] with the same color).
    Premultiplied,
}

impl ParticleBlend {
    /// Every blend mode, in its stable declaration order.
    pub const ALL: [Self; 3] = [Self::Alpha, Self::Additive, Self::Premultiplied];

    /// The serialized spelling (`"alpha"`, `"additive"`, or `"premultiplied"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Alpha => "alpha",
            Self::Additive => "additive",
            Self::Premultiplied => "premultiplied",
        }
    }

    /// Parse a serialized spelling; matching is exact and case-sensitive.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|blend| blend.as_str() == text)
    }
}

/// How an emitter's particles respond to the scene's lights (the emitter `lighting` field; omitted means
/// [`ParticleLighting::Unlit`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ParticleLighting {
    /// The particle shows its color as authored, whatever lights the scene (the default and the original look): suits
    /// fire, sparks, and anything that emits light.
    #[default]
    Unlit,
    /// The particle's color is lit by the frame's ambient, skylight, directional, and point lights with a cheap
    /// camera-facing half-Lambert model, without shadows: suits smoke, dust, and debris that should sit in the scene's
    /// lighting.
    Lit,
}

impl ParticleLighting {
    /// Every lighting mode, in its stable declaration order.
    pub const ALL: [Self; 2] = [Self::Unlit, Self::Lit];

    /// The serialized spelling (`"unlit"` or `"lit"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unlit => "unlit",
            Self::Lit => "lit",
        }
    }

    /// Parse a serialized spelling; matching is exact and case-sensitive.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lighting| lighting.as_str() == text)
    }
}

/// Serialized effect parameter types (the parameter `type` field; omitted means scalar).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleParameterType {
    /// One number.
    Scalar,
    /// Three numbers exposed to scripts as `nameX`, `nameY`, `nameZ` and the vector `name`.
    Vector3,
    /// Four linear numbers (nonnegative RGB that may exceed one, alpha in `[0, 1]`) exposed to scripts as the color
    /// `name`.
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
#[path = "constants.test.rs"]
mod tests;
