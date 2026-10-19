// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Material records: the metallic-roughness material and its texture sources, with glTF-compatible alpha modes.

use serde::{
    Deserialize,
    Serialize,
};

/// glTF-compatible material alpha modes, serialized as `OPAQUE`, `MASK`, or `BLEND`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MaterialAlphaMode {
    /// Alpha is ignored.
    #[default]
    Opaque,
    /// Fragments below the alpha cutoff are discarded.
    Mask,
    /// Alpha blends the surface over what is behind it.
    Blend,
}

/// Borrowed texture sources (URLs) for one PBR material; the renderer owns decoded GPU textures.
///
/// Color maps use sRGB; normal and scalar maps use linear data (scalar maps read the red channel).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbrMaterialTextures {
    /// Base color map.
    pub base_color: Option<String>,
    /// Tangent-space normal map.
    pub normal: Option<String>,
    /// Emissive map.
    pub emissive: Option<String>,
    /// Metallic map.
    pub metallic: Option<String>,
    /// Roughness map.
    pub roughness: Option<String>,
    /// Ambient occlusion map.
    pub ao: Option<String>,
}

/// Metallic-roughness material. Factors are linear; colors have RGB or RGBA order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbrMaterial {
    /// Linear RGB or RGBA base color factor.
    pub base_color: Vec<f64>,
    /// Linear RGB emissive factor.
    pub emissive: Vec<f64>,
    /// Metalness factor in `[0, 1]`.
    pub metallic: f64,
    /// Perceptual roughness factor in `[0, 1]`.
    pub roughness: f64,
    /// Alpha handling.
    pub alpha_mode: MaterialAlphaMode,
    /// Alpha threshold for [`MaterialAlphaMode::Mask`].
    pub alpha_cutoff: f64,
    /// Optional texture sources.
    pub textures: PbrMaterialTextures,
}
