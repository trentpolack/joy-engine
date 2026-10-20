// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime debug views: draw one surface attribute (or one stage of the frame) instead of the lit scene.
//!
//! Surface views come from the attributes every shading model reports ([`SurfaceAttributes`] in
//! `surface-contract.wgsl`), so they work for every model, built in or registered. They replace mesh colors in the scene
//! pass, which shades forward while a debug view is on; terrain, the sky, and particles keep drawing normally around
//! them. Display processing (exposure and tone
//! mapping) still applies, so compare views under a neutral look when exact values matter.

use serde::{
    Deserialize,
    Serialize,
};

/// What the scene pass draws for mesh surfaces. Serialized in camelCase for console variables and saved settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DebugView {
    /// The lit scene.
    #[default]
    Lit,
    /// Diffuse albedo for dielectrics and specular color for metals.
    BaseColor,
    /// Diffuse albedo alone.
    DiffuseAlbedo,
    /// Specular reflectance at normal incidence (F0).
    SpecularColor,
    /// Perceptual roughness as gray.
    Roughness,
    /// Metallic amount as gray.
    Metallic,
    /// World-space shading normals mapped from `[-1, 1]` to `[0, 1]`.
    Normals,
    /// Emitted radiance.
    Emissive,
    /// Ambient occlusion as gray.
    AmbientOcclusion,
    /// The light a white, diffuse surface receives (shadows and skylight included).
    LightingOnly,
    /// A distinct color per shading model.
    ShadingModel,
    /// Directional shadow cascades, tinted per cascade over the lighting.
    ShadowCascades,
    /// Texture coordinates with an eight-by-eight checker.
    TextureCoordinates,
    /// How many point lights the light cluster under each surface lists, as a heat map from blue (one) to red
    /// (32 or more); black where none reach.
    LightComplexity,
}

impl DebugView {
    /// Every view, in menu order.
    pub const ALL: [Self; 14] = [
        Self::Lit,
        Self::BaseColor,
        Self::DiffuseAlbedo,
        Self::SpecularColor,
        Self::Roughness,
        Self::Metallic,
        Self::Normals,
        Self::Emissive,
        Self::AmbientOcclusion,
        Self::LightingOnly,
        Self::ShadingModel,
        Self::ShadowCascades,
        Self::TextureCoordinates,
        Self::LightComplexity,
    ];

    /// A short label for menus.
    pub fn label(self) -> &'static str {
        match self {
            Self::Lit => "Lit",
            Self::BaseColor => "Base color",
            Self::DiffuseAlbedo => "Diffuse albedo",
            Self::SpecularColor => "Specular color",
            Self::Roughness => "Roughness",
            Self::Metallic => "Metallic",
            Self::Normals => "Normals",
            Self::Emissive => "Emissive",
            Self::AmbientOcclusion => "Ambient occlusion",
            Self::LightingOnly => "Lighting only",
            Self::ShadingModel => "Shading model",
            Self::ShadowCascades => "Shadow cascades",
            Self::TextureCoordinates => "Texture coordinates",
            Self::LightComplexity => "Light complexity",
        }
    }

    /// The view's camelCase name, as console variables and saved settings spell it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Lit => "lit",
            Self::BaseColor => "baseColor",
            Self::DiffuseAlbedo => "diffuseAlbedo",
            Self::SpecularColor => "specularColor",
            Self::Roughness => "roughness",
            Self::Metallic => "metallic",
            Self::Normals => "normals",
            Self::Emissive => "emissive",
            Self::AmbientOcclusion => "ambientOcclusion",
            Self::LightingOnly => "lightingOnly",
            Self::ShadingModel => "shadingModel",
            Self::ShadowCascades => "shadowCascades",
            Self::TextureCoordinates => "textureCoordinates",
            Self::LightComplexity => "lightComplexity",
        }
    }

    /// The view a [`name`](Self::name) spells, ignoring case.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.name().eq_ignore_ascii_case(name))
    }

    /// The view's index in the scene shaders (`DEBUG_*` in `surface-scene.wgsl`).
    pub(crate) fn shader_index(self) -> u32 {
        Self::ALL.iter().position(|view| *view == self).unwrap_or(0) as u32
    }
}
