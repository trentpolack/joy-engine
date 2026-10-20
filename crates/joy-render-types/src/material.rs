// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Materials: how a surface responds to light, independent of any mesh and of any GPU.
//!
//! A [`Material`] pairs a [`Shading`] (a shading model and its inputs) with the coverage rules every model shares
//! (alpha mode and cutoff). Meshes reference materials only through slots, so one mesh can be drawn with any material
//! and any shading model.
//!
//! The engine's built-in models:
//! - [`Shading::Layered`]: physically based surfaces built from [`SurfaceLayer`]s. A layer describes its reflectance
//!   with a diffuse albedo, specular reflectance at normal incidence (F0) and at grazing angles (F90), and roughness,
//!   plus optional haze, fuzz, coat, and thin-film features. [`Surface`] operators stack (coat), mix (blend), add, and
//!   weight layers; the renderer collapses each tree into one layer by blending parameters
//!   ([`Surface::collapse`]), so a layered material costs one layer to shade.
//! - [`Shading::Lit`]: the engine's flat, faceted, double-sided stylized lighting.
//! - [`Shading::Unlit`]: color without lighting.
//! - [`Shading::Custom`]: a model a game registers with the renderer, with named parameters and textures.
//!
//! Colors are linear light; texture sources are URLs or `data:` URLs that the renderer resolves when it builds a GPU
//! material. Records serialize with camelCase names and glTF alpha-mode spellings.

use indexmap::IndexMap;
use serde::{
    Deserialize,
    Serialize,
};

use joy_core::LinearColor;
use joy_core::color::rgb_array;

/// A texture's location: a URL, a project path, or an embedded `data:image/…;base64,` URL.
pub type TextureSource = String;

/// The default alpha cutoff for [`AlphaMode::Mask`], glTF's.
pub const DEFAULT_ALPHA_CUTOFF: f32 = 0.5;

/// Reflectance at normal incidence of common dielectrics (index of refraction 1.5), as an RGB F0.
pub const DIELECTRIC_F0: f32 = 0.04;

// ---------------------------------------------------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------------------------------------------------

/// How a material's alpha affects coverage; serialized with glTF's spellings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AlphaMode {
    /// Alpha is ignored; the surface is fully opaque.
    #[default]
    Opaque,
    /// Fragments whose alpha is below the cutoff are discarded; the rest are opaque.
    Mask,
    /// Alpha blends the surface over what is behind it (drawn sorted, back to front).
    Blend,
}

/// A complete material: a shading model with its inputs, and the coverage rules every model shares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    /// Name for tools and slot matching (FORM palette names, glTF material names); may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The shading model and its inputs.
    pub shading: Shading,
    /// How alpha affects coverage.
    #[serde(default)]
    pub alpha_mode: AlphaMode,
    /// Alpha below which [`AlphaMode::Mask`] discards a fragment, in `[0, 1]`.
    #[serde(default = "default_alpha_cutoff")]
    pub alpha_cutoff: f32,
}

impl Default for Material {
    /// The engine's standard look: opaque white [`Shading::Lit`].
    fn default() -> Self {
        Self::new(Shading::Lit(LitSurface::default()))
    }
}

impl Material {
    /// An opaque, unnamed material with `shading`.
    pub fn new(shading: Shading) -> Self {
        Self {
            name: String::new(),
            shading,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: DEFAULT_ALPHA_CUTOFF,
        }
    }

    /// An opaque [`Shading::Lit`] material tinted `base_color`.
    pub fn lit(base_color: LinearColor) -> Self {
        Self::new(Shading::Lit(LitSurface {
            base_color,
            ..LitSurface::default()
        }))
    }

    /// An opaque [`Shading::Unlit`] material of `color`.
    pub fn unlit(color: LinearColor) -> Self {
        Self::new(Shading::Unlit(UnlitSurface {
            base_color: color,
            ..UnlitSurface::default()
        }))
    }

    /// An opaque [`Shading::Layered`] material of one layer.
    pub fn layered(layer: SurfaceLayer) -> Self {
        Self::new(Shading::Layered(Surface::Layer(layer)))
    }

    /// This material renamed.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// This material with an alpha mode (and the default cutoff kept).
    pub fn with_alpha_mode(mut self, alpha_mode: AlphaMode) -> Self {
        self.alpha_mode = alpha_mode;
        self
    }

    /// The shading model this material uses.
    pub fn model(&self) -> ShadingModel<'_> {
        self.shading.model()
    }

    /// Whether the material draws in the sorted transparent queue.
    pub fn blends(&self) -> bool {
        self.alpha_mode == AlphaMode::Blend
    }

    /// Whether depth-only passes must alpha-test the material.
    pub fn masks(&self) -> bool {
        self.alpha_mode == AlphaMode::Mask
    }

    /// Every texture source the material samples, with the name the shading model binds it to.
    pub fn texture_sources(&self) -> Vec<(&str, &TextureSource)> {
        self.shading.texture_sources()
    }

    /// Check that every value is usable: finite, nonnegative colors, factors and opacity in `[0, 1]`, nonempty texture
    /// sources, and a cutoff in `[0, 1]`.
    pub fn validate(&self) -> Result<(), MaterialError> {
        if !(0.0..=1.0).contains(&self.alpha_cutoff) {
            return Err(MaterialError::OutOfRange("alphaCutoff"));
        }
        if self.texture_sources().iter().any(|(_, source)| source.trim().is_empty()) {
            return Err(MaterialError::EmptyTextureSource);
        }
        self.shading.validate()
    }
}

fn default_alpha_cutoff() -> f32 {
    DEFAULT_ALPHA_CUTOFF
}

// ---------------------------------------------------------------------------------------------------------------------
// Shading models
// ---------------------------------------------------------------------------------------------------------------------

/// The engine's built-in shading models, plus custom models a game registers by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShadingModel<'a> {
    /// Physically based layered surfaces ([`Shading::Layered`]).
    Layered,
    /// Flat, faceted, double-sided stylized lighting ([`Shading::Lit`]).
    Lit,
    /// Color without lighting ([`Shading::Unlit`]).
    Unlit,
    /// A registered custom model ([`Shading::Custom`]), by name.
    Custom(&'a str),
}

impl ShadingModel<'_> {
    /// The model's registry name: `layered`, `lit`, `unlit`, or the custom model's name.
    pub fn name(&self) -> &str {
        match self {
            Self::Layered => LAYERED_MODEL,
            Self::Lit => LIT_MODEL,
            Self::Unlit => UNLIT_MODEL,
            Self::Custom(name) => name,
        }
    }
}

/// Registry name of [`ShadingModel::Layered`].
pub const LAYERED_MODEL: &str = "layered";

/// Registry name of [`ShadingModel::Lit`].
pub const LIT_MODEL: &str = "lit";

/// Registry name of [`ShadingModel::Unlit`].
pub const UNLIT_MODEL: &str = "unlit";

/// A shading model with its inputs; serialized as `{"layered": …}`, `{"lit": …}`, `{"unlit": …}`, or `{"custom": …}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shading {
    /// Physically based layered surfaces.
    Layered(Surface),
    /// The engine's flat, faceted, double-sided stylized lighting.
    Lit(LitSurface),
    /// Color without lighting.
    Unlit(UnlitSurface),
    /// A model a game registers with the renderer.
    Custom(CustomShading),
}

impl Shading {
    /// The shading model.
    pub fn model(&self) -> ShadingModel<'_> {
        match self {
            Self::Layered(_) => ShadingModel::Layered,
            Self::Lit(_) => ShadingModel::Lit,
            Self::Unlit(_) => ShadingModel::Unlit,
            Self::Custom(custom) => ShadingModel::Custom(&custom.model),
        }
    }

    /// Every texture source the inputs sample, with the binding name the model reads it through.
    pub fn texture_sources(&self) -> Vec<(&str, &TextureSource)> {
        match self {
            Self::Layered(surface) => {
                let mut sources = Vec::new();
                surface.visit_layers(&mut |layer| sources.extend(layer.maps.sources()));
                sources
            }
            Self::Lit(lit) => lit.base_color_map.iter().map(|source| (BASE_COLOR_MAP, source)).collect(),
            Self::Unlit(unlit) => unlit.base_color_map.iter().map(|source| (BASE_COLOR_MAP, source)).collect(),
            Self::Custom(custom) => custom.textures.iter().map(|(name, source)| (name.as_str(), source)).collect(),
        }
    }

    fn validate(&self) -> Result<(), MaterialError> {
        match self {
            Self::Layered(surface) => {
                let mut result = Ok(());
                surface.visit_layers(&mut |layer| {
                    if result.is_ok() {
                        result = layer.validate();
                    }
                });
                result?;
                surface.validate_operators()
            }
            Self::Lit(lit) => {
                check_color("baseColor", lit.base_color, true)?;
                check_color("emissive", lit.emissive, false)
            }
            Self::Unlit(unlit) => check_color("baseColor", unlit.base_color, true),
            Self::Custom(custom) => custom.validate(),
        }
    }
}

/// Binding name of a base color map, shared by every built-in model.
pub const BASE_COLOR_MAP: &str = "baseColor";

/// Binding name of a tangent-space normal map.
pub const NORMAL_MAP: &str = "normal";

/// Binding name of an emissive map.
pub const EMISSIVE_MAP: &str = "emissive";

/// Binding name of a metallic map.
pub const METALLIC_MAP: &str = "metallic";

/// Binding name of a roughness map.
pub const ROUGHNESS_MAP: &str = "roughness";

/// Binding name of an ambient occlusion map.
pub const OCCLUSION_MAP: &str = "occlusion";

// ---------------------------------------------------------------------------------------------------------------------
// Layered surfaces
// ---------------------------------------------------------------------------------------------------------------------

/// One physically based surface layer: an interface that reflects specularly, over material that scatters diffusely.
///
/// Reflectance is described by a diffuse albedo and the specular reflectance at normal incidence (`f0`) and at grazing
/// angles (`f90`). `metallic` blends toward a conductor whose `base_color` is its F0 and which has no diffuse
/// response: at one, `base_color` replaces `f0`, `f90` becomes white, and diffuse reflection disappears. A layer with
/// `metallic` zero is a dielectric whose diffuse albedo is `base_color`. Both descriptions meet in one layer so maps
/// authored for the metallic workflow (glTF, FORM) and direct F0/F90 authoring share the same model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SurfaceLayer {
    /// Diffuse albedo for dielectrics and F0 for metals, with straight alpha as opacity.
    pub base_color: LinearColor,
    /// Conductor amount in `[0, 1]`.
    pub metallic: f32,
    /// Dielectric specular reflectance at normal incidence, RGB in `[0, 1]`; 0.04 for most nonmetals.
    #[serde(with = "rgb_array")]
    pub f0: LinearColor,
    /// Dielectric specular reflectance at grazing angles, RGB in `[0, 1]`; white for physical surfaces.
    #[serde(with = "rgb_array")]
    pub f90: LinearColor,
    /// Perceptual roughness in `[0, 1]`.
    pub roughness: f32,
    /// Emitted linear radiance, RGB; may exceed one for HDR glow.
    #[serde(with = "rgb_array")]
    pub emissive: LinearColor,
    /// A second, rougher specular lobe blended into the first (hazy gloss).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haze: Option<Haze>,
    /// Retro-reflective sheen from fibers (cloth, velvet, dust).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuzz: Option<Fuzz>,
    /// A clear dielectric coat over the layer (lacquer, car paint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coat: Option<Coat>,
    /// A thin film on the surface that produces iridescence (soap bubbles, oil slicks, heat tint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thin_film: Option<ThinFilm>,
    /// Texture maps that modulate the factors above.
    #[serde(skip_serializing_if = "SurfaceMaps::is_empty")]
    pub maps: SurfaceMaps,
}

impl Default for SurfaceLayer {
    /// A white, moderately rough dielectric.
    fn default() -> Self {
        Self {
            base_color: LinearColor::WHITE,
            metallic: 0.0,
            f0: LinearColor::rgb(DIELECTRIC_F0, DIELECTRIC_F0, DIELECTRIC_F0),
            f90: LinearColor::WHITE,
            roughness: 0.5,
            emissive: LinearColor::BLACK,
            haze: None,
            fuzz: None,
            coat: None,
            thin_film: None,
            maps: SurfaceMaps::default(),
        }
    }
}

impl SurfaceLayer {
    /// A dielectric with `diffuse_albedo` and the common 4% specular reflectance.
    pub fn dielectric(diffuse_albedo: LinearColor, roughness: f32) -> Self {
        Self {
            base_color: diffuse_albedo,
            roughness,
            ..Self::default()
        }
    }

    /// A conductor whose specular reflectance at normal incidence is `f0`.
    pub fn metal(f0: LinearColor, roughness: f32) -> Self {
        Self {
            base_color: f0,
            metallic: 1.0,
            roughness,
            ..Self::default()
        }
    }

    /// A layer from glTF's metallic-roughness factors.
    pub fn metallic_roughness(base_color: LinearColor, metallic: f32, roughness: f32) -> Self {
        Self {
            base_color,
            metallic,
            roughness,
            ..Self::default()
        }
    }

    /// The diffuse albedo this layer scatters (black for metals).
    pub fn diffuse_albedo(&self) -> LinearColor {
        self.base_color.scaled(1.0 - self.metallic).with_alpha(1.0)
    }

    /// The specular reflectance at normal incidence, after the metallic blend.
    pub fn specular_f0(&self) -> LinearColor {
        mix_color(self.f0, self.base_color, self.metallic).with_alpha(1.0)
    }

    fn validate(&self) -> Result<(), MaterialError> {
        check_color("baseColor", self.base_color, true)?;
        check_unit("metallic", self.metallic)?;
        check_unit_color("f0", self.f0)?;
        check_unit_color("f90", self.f90)?;
        check_unit("roughness", self.roughness)?;
        check_color("emissive", self.emissive, false)?;
        if let Some(haze) = self.haze {
            check_unit("haze.roughness", haze.roughness)?;
            check_unit("haze.weight", haze.weight)?;
        }
        if let Some(fuzz) = self.fuzz {
            check_unit_color("fuzz.color", fuzz.color)?;
            check_unit("fuzz.roughness", fuzz.roughness)?;
            check_unit("fuzz.amount", fuzz.amount)?;
        }
        if let Some(coat) = self.coat {
            check_unit("coat.weight", coat.weight)?;
            check_unit("coat.roughness", coat.roughness)?;
            check_unit("coat.f0", coat.f0)?;
            check_unit_color("coat.tint", coat.tint)?;
        }
        if let Some(film) = self.thin_film {
            if !(film.thickness.is_finite() && (0.0..=MAX_THIN_FILM_THICKNESS).contains(&film.thickness)) {
                return Err(MaterialError::OutOfRange("thinFilm.thickness"));
            }
            if !(film.ior.is_finite() && (1.0..=MAX_THIN_FILM_IOR).contains(&film.ior)) {
                return Err(MaterialError::OutOfRange("thinFilm.ior"));
            }
            check_unit("thinFilm.weight", film.weight)?;
        }
        Ok(())
    }
}

/// A second specular lobe blended into a layer's primary lobe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Haze {
    /// Perceptual roughness of the hazy lobe in `[0, 1]`, usually rougher than the layer.
    pub roughness: f32,
    /// How much of the specular response comes from the hazy lobe, in `[0, 1]`.
    pub weight: f32,
}

/// Sheen from fibers standing off a surface, brightest at grazing angles.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fuzz {
    /// Linear sheen color, RGB in `[0, 1]`.
    #[serde(with = "rgb_array")]
    pub color: LinearColor,
    /// Fiber roughness in `[0, 1]`; low values give a tight rim.
    pub roughness: f32,
    /// Fiber coverage in `[0, 1]`.
    pub amount: f32,
}

/// A clear dielectric coat: its own specular lobe, which dims and tints the layer beneath it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coat {
    /// Coat coverage in `[0, 1]`.
    pub weight: f32,
    /// Perceptual roughness of the coat in `[0, 1]`.
    pub roughness: f32,
    /// The coat's reflectance at normal incidence in `[0, 1]`; 0.04 for lacquer.
    pub f0: f32,
    /// Transmission tint light picks up passing through the coat, RGB in `[0, 1]`; white for a clear coat.
    #[serde(with = "rgb_array")]
    pub tint: LinearColor,
}

impl Default for Coat {
    /// A full, glossy, clear lacquer.
    fn default() -> Self {
        Self {
            weight: 1.0,
            roughness: 0.05,
            f0: DIELECTRIC_F0,
            tint: LinearColor::WHITE,
        }
    }
}

/// Thickest supported thin film, in nanometers.
pub const MAX_THIN_FILM_THICKNESS: f32 = 2000.0;

/// Largest supported thin-film index of refraction.
pub const MAX_THIN_FILM_IOR: f32 = 3.0;

/// A thin dielectric film whose interference shifts specular color with viewing angle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinFilm {
    /// Film thickness in nanometers, up to [`MAX_THIN_FILM_THICKNESS`]; visible colors appear between roughly 100 and
    /// 1000.
    pub thickness: f32,
    /// The film's index of refraction, from 1 to [`MAX_THIN_FILM_IOR`].
    pub ior: f32,
    /// How strongly the film replaces the layer's own specular color, in `[0, 1]`.
    pub weight: f32,
}

/// Texture maps for a [`SurfaceLayer`]. Missing maps leave their factors unmodulated.
///
/// Color maps (base color and emissive) hold sRGB values; the normal map holds tangent-space XYZ; metallic, roughness,
/// and occlusion maps read their red channel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceMaps {
    /// Multiplies `base_color`, alpha included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<TextureSource>,
    /// Tangent-space normal perturbation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<TextureSource>,
    /// Multiplies `emissive`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emissive: Option<TextureSource>,
    /// Multiplies `metallic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<TextureSource>,
    /// Multiplies `roughness`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<TextureSource>,
    /// Ambient occlusion applied to indirect light.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occlusion: Option<TextureSource>,
}

impl SurfaceMaps {
    /// Whether no map is set.
    pub fn is_empty(&self) -> bool {
        self.sources().next().is_none()
    }

    /// Each set map with its binding name.
    pub fn sources(&self) -> impl Iterator<Item = (&'static str, &TextureSource)> {
        [
            (BASE_COLOR_MAP, &self.base_color),
            (NORMAL_MAP, &self.normal),
            (EMISSIVE_MAP, &self.emissive),
            (METALLIC_MAP, &self.metallic),
            (ROUGHNESS_MAP, &self.roughness),
            (OCCLUSION_MAP, &self.occlusion),
        ]
        .into_iter()
        .filter_map(|(name, source)| source.as_ref().map(|source| (name, source)))
    }
}

/// A tree of surface layers combined by operators.
///
/// The operators follow layered-material practice: `Coat` stacks one surface over another (light reaching the lower
/// surface passes through the upper one), `Blend` mixes two surfaces side by side, `Add` sums their responses (not
/// energy conserving; for stylized effects), and `Weight` scales a surface's coverage. [`Surface::collapse`] blends
/// parameters into one equivalent layer, which is how the renderer shades every tree today; a tree's serialized form
/// stays the same if a renderer later evaluates layers separately.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
// Trees are small and built once per material, so leaves stay inline rather than adding a box to every layer.
#[allow(clippy::large_enum_variant)]
pub enum Surface {
    /// A single layer.
    Layer(SurfaceLayer),
    /// `coat` stacked over `base`: the coat's specular response sits on top and tints what passes through it.
    Coat {
        /// The upper surface.
        coat: Box<Surface>,
        /// The lower surface.
        base: Box<Surface>,
    },
    /// `first` and `second` mixed side by side, `weight` of the way toward `second`.
    Blend {
        /// The surface at weight zero.
        first: Box<Surface>,
        /// The surface at weight one.
        second: Box<Surface>,
        /// Mix toward `second` in `[0, 1]`.
        weight: f32,
    },
    /// The sum of two surfaces' responses.
    Add {
        /// One surface.
        first: Box<Surface>,
        /// The other surface.
        second: Box<Surface>,
    },
    /// `surface` with its coverage scaled by `weight`.
    Weight {
        /// The weighted surface.
        surface: Box<Surface>,
        /// Coverage in `[0, 1]`.
        weight: f32,
    },
}

impl Default for Surface {
    fn default() -> Self {
        Self::Layer(SurfaceLayer::default())
    }
}

impl Surface {
    /// Blend this tree's parameters into one equivalent layer.
    ///
    /// Scalar and color parameters interpolate (blend), sum (add), or scale (weight). A coat becomes the result's coat
    /// lobe: the upper surface's roughness and F0 shape the coat, and its base color tints what passes through it.
    /// Texture maps cannot be blended as parameters, so a blend keeps the maps of whichever side has more weight.
    pub fn collapse(&self) -> SurfaceLayer {
        match self {
            Self::Layer(layer) => layer.clone(),
            Self::Coat {
                coat,
                base,
            } => coat_layers(&coat.collapse(), base.collapse()),
            Self::Blend {
                first,
                second,
                weight,
            } => blend_layers(&first.collapse(), &second.collapse(), *weight),
            Self::Add {
                first,
                second,
            } => add_layers(&first.collapse(), &second.collapse()),
            Self::Weight {
                surface,
                weight,
            } => weight_layer(surface.collapse(), *weight),
        }
    }

    /// Call `visit` for every leaf layer, depth first.
    pub fn visit_layers<'a>(&'a self, visit: &mut impl FnMut(&'a SurfaceLayer)) {
        match self {
            Self::Layer(layer) => visit(layer),
            Self::Coat {
                coat,
                base,
            } => {
                coat.visit_layers(visit);
                base.visit_layers(visit);
            }
            Self::Blend {
                first,
                second,
                ..
            }
            | Self::Add {
                first,
                second,
            } => {
                first.visit_layers(visit);
                second.visit_layers(visit);
            }
            Self::Weight {
                surface,
                ..
            } => surface.visit_layers(visit),
        }
    }

    fn validate_operators(&self) -> Result<(), MaterialError> {
        match self {
            Self::Layer(_) => Ok(()),
            Self::Coat {
                coat,
                base,
            } => {
                coat.validate_operators()?;
                base.validate_operators()
            }
            Self::Blend {
                first,
                second,
                weight,
            } => {
                check_unit("blend.weight", *weight)?;
                first.validate_operators()?;
                second.validate_operators()
            }
            Self::Add {
                first,
                second,
            } => {
                first.validate_operators()?;
                second.validate_operators()
            }
            Self::Weight {
                surface,
                weight,
            } => {
                check_unit("weight.weight", *weight)?;
                surface.validate_operators()
            }
        }
    }
}

/// `upper` collapsed into `base`'s coat lobe.
fn coat_layers(upper: &SurfaceLayer, mut base: SurfaceLayer) -> SurfaceLayer {
    let f0 = upper.specular_f0();
    let tint = upper.base_color.with_alpha(1.0);
    base.coat = Some(Coat {
        weight: upper.base_color.alpha,
        roughness: upper.roughness,
        f0: luminance(f0).clamp(0.0, 1.0),
        tint: LinearColor::rgb(tint.red.min(1.0), tint.green.min(1.0), tint.blue.min(1.0)),
    });
    let emitted = base.emissive.to_array();
    let tinted = [emitted[0] * tint.red, emitted[1] * tint.green, emitted[2] * tint.blue];
    base.emissive = LinearColor::rgb(
        tinted[0] + upper.emissive.red,
        tinted[1] + upper.emissive.green,
        tinted[2] + upper.emissive.blue,
    );
    base
}

/// `first` and `second` interpolated by `weight`.
fn blend_layers(first: &SurfaceLayer, second: &SurfaceLayer, weight: f32) -> SurfaceLayer {
    let weight = weight.clamp(0.0, 1.0);
    let mix = |from: f32, to: f32| from + ((to - from) * weight);
    SurfaceLayer {
        base_color: mix_color(first.base_color, second.base_color, weight),
        metallic: mix(first.metallic, second.metallic),
        f0: mix_color(first.f0, second.f0, weight),
        f90: mix_color(first.f90, second.f90, weight),
        roughness: mix(first.roughness, second.roughness),
        emissive: mix_color(first.emissive, second.emissive, weight),
        haze: blend_optional(
            first.haze,
            second.haze,
            |haze| Haze {
                weight: 0.0,
                ..haze
            },
            |from, to| Haze {
                roughness: mix(from.roughness, to.roughness),
                weight: mix(from.weight, to.weight),
            },
        ),
        fuzz: blend_optional(
            first.fuzz,
            second.fuzz,
            |fuzz| Fuzz {
                amount: 0.0,
                ..fuzz
            },
            |from, to| Fuzz {
                color: mix_color(from.color, to.color, weight),
                roughness: mix(from.roughness, to.roughness),
                amount: mix(from.amount, to.amount),
            },
        ),
        coat: blend_optional(
            first.coat,
            second.coat,
            |coat| Coat {
                weight: 0.0,
                ..coat
            },
            |from, to| Coat {
                weight: mix(from.weight, to.weight),
                roughness: mix(from.roughness, to.roughness),
                f0: mix(from.f0, to.f0),
                tint: mix_color(from.tint, to.tint, weight),
            },
        ),
        thin_film: blend_optional(
            first.thin_film,
            second.thin_film,
            |film| ThinFilm {
                weight: 0.0,
                ..film
            },
            |from, to| ThinFilm {
                thickness: mix(from.thickness, to.thickness),
                ior: mix(from.ior, to.ior),
                weight: mix(from.weight, to.weight),
            },
        ),
        maps: if weight < 0.5 { first.maps.clone() } else { second.maps.clone() },
    }
}

/// The responses of `first` and `second` summed. Scalars that are not responses average.
fn add_layers(first: &SurfaceLayer, second: &SurfaceLayer) -> SurfaceLayer {
    let sum = |left: LinearColor, right: LinearColor| {
        LinearColor::new(
            left.red + right.red,
            left.green + right.green,
            left.blue + right.blue,
            left.alpha.max(right.alpha),
        )
    };
    SurfaceLayer {
        base_color: sum(first.base_color, second.base_color),
        f0: clamp_unit_color(sum(first.f0, second.f0)),
        emissive: sum(first.emissive, second.emissive),
        ..blend_layers(first, second, 0.5)
    }
}

/// `layer` with its coverage and responses scaled by `weight`.
fn weight_layer(mut layer: SurfaceLayer, weight: f32) -> SurfaceLayer {
    let weight = weight.clamp(0.0, 1.0);
    layer.base_color = layer.base_color.with_alpha(layer.base_color.alpha * weight);
    layer.emissive = layer.emissive.scaled(weight);
    layer
}

/// Interpolate two optional lobes, treating a missing lobe as the other one with zero weight.
fn blend_optional<T: Copy>(
    first: Option<T>,
    second: Option<T>,
    absent: impl Fn(T) -> T,
    mix: impl Fn(T, T) -> T,
) -> Option<T> {
    match (first, second) {
        (Some(from), Some(to)) => Some(mix(from, to)),
        (Some(from), None) => Some(mix(from, absent(from))),
        (None, Some(to)) => Some(mix(absent(to), to)),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Stylized and custom models
// ---------------------------------------------------------------------------------------------------------------------

/// Inputs to the engine's flat, faceted, double-sided stylized lighting.
///
/// Faces light by their geometric (screen-derivative) normals, so meshes read as faceted however their normals were
/// authored; diffuse terms omit the 1/π factor so authored colors keep their brightness under unit light.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LitSurface {
    /// Linear color with straight alpha; multiplies vertex colors.
    pub base_color: LinearColor,
    /// Emitted linear radiance, RGB; may exceed one.
    #[serde(with = "rgb_array")]
    pub emissive: LinearColor,
    /// Multiplies `base_color`, alpha included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_color_map: Option<TextureSource>,
}

impl Default for LitSurface {
    fn default() -> Self {
        Self {
            base_color: LinearColor::WHITE,
            emissive: LinearColor::BLACK,
            base_color_map: None,
        }
    }
}

/// Inputs to unlit shading: the surface shows its color unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UnlitSurface {
    /// Linear color with straight alpha; multiplies vertex colors. RGB may exceed one.
    pub base_color: LinearColor,
    /// Multiplies `base_color`, alpha included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_color_map: Option<TextureSource>,
}

impl Default for UnlitSurface {
    fn default() -> Self {
        Self {
            base_color: LinearColor::WHITE,
            base_color_map: None,
        }
    }
}

/// One custom-model parameter value: a number, or a vector of two to four components (colors are four).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShaderValue {
    /// A scalar.
    Number(f32),
    /// Two to four components.
    Vector(Vec<f32>),
}

impl ShaderValue {
    /// The value padded to four components with zeros.
    pub fn to_vec4(&self) -> [f32; 4] {
        match self {
            Self::Number(value) => [*value, 0.0, 0.0, 0.0],
            Self::Vector(values) => std::array::from_fn(|index| values.get(index).copied().unwrap_or(0.0)),
        }
    }
}

impl From<f32> for ShaderValue {
    fn from(value: f32) -> Self {
        Self::Number(value)
    }
}

impl From<LinearColor> for ShaderValue {
    fn from(color: LinearColor) -> Self {
        Self::Vector(color.to_array().to_vec())
    }
}

/// Inputs to a model a game registers with the renderer: the model's name and its named parameters and textures.
///
/// The renderer checks names, types, and texture bindings against the registered model when it builds the GPU
/// material; parameters the material omits take the model's defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomShading {
    /// The registered model's name.
    pub model: String,
    /// Parameter values by name.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub parameters: IndexMap<String, ShaderValue>,
    /// Texture sources by binding name.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub textures: IndexMap<String, TextureSource>,
}

impl CustomShading {
    fn validate(&self) -> Result<(), MaterialError> {
        if self.model.trim().is_empty() {
            return Err(MaterialError::UnnamedCustomModel);
        }
        let usable = |value: &ShaderValue| match value {
            ShaderValue::Number(number) => number.is_finite(),
            ShaderValue::Vector(values) => {
                (2..=4).contains(&values.len()) && values.iter().all(|value| value.is_finite())
            }
        };
        match self.parameters.iter().find(|(_, value)| !usable(value)) {
            Some(_) => Err(MaterialError::InvalidParameter),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------------------------------------------------

/// Why a material cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MaterialError {
    /// A color has a negative or non-finite channel (or an opacity outside `[0, 1]`).
    #[error("Material {0} must have finite, nonnegative channels and an opacity between 0 and 1.")]
    InvalidColor(&'static str),
    /// A factor is non-finite or outside its range.
    #[error("Material {0} is outside its range.")]
    OutOfRange(&'static str),
    /// A texture source is empty.
    #[error("Material texture sources must be nonempty.")]
    EmptyTextureSource,
    /// A custom shading model has no name.
    #[error("Custom shading must name its model.")]
    UnnamedCustomModel,
    /// A custom parameter is non-finite or has the wrong number of components.
    #[error("Custom shading parameters must be finite numbers or vectors of two to four components.")]
    InvalidParameter,
}

/// Reject a color with negative or non-finite channels; `opacity` also requires alpha in `[0, 1]`.
fn check_color(name: &'static str, color: LinearColor, opacity: bool) -> Result<(), MaterialError> {
    let channels_valid = color.rgb_array().iter().all(|channel| channel.is_finite() && *channel >= 0.0);
    let alpha_valid = !opacity || (0.0..=1.0).contains(&color.alpha);
    if channels_valid && alpha_valid { Ok(()) } else { Err(MaterialError::InvalidColor(name)) }
}

/// Reject a color whose channels leave `[0, 1]`.
fn check_unit_color(name: &'static str, color: LinearColor) -> Result<(), MaterialError> {
    if color.rgb_array().iter().all(|channel| (0.0..=1.0).contains(channel)) {
        Ok(())
    } else {
        Err(MaterialError::OutOfRange(name))
    }
}

/// Reject a factor outside `[0, 1]` (NaN included).
fn check_unit(name: &'static str, value: f32) -> Result<(), MaterialError> {
    if (0.0..=1.0).contains(&value) { Ok(()) } else { Err(MaterialError::OutOfRange(name)) }
}

/// Linear interpolation between two colors, alpha included.
fn mix_color(from: LinearColor, to: LinearColor, weight: f32) -> LinearColor {
    let mix = |left: f32, right: f32| left + ((right - left) * weight);
    LinearColor::new(
        mix(from.red, to.red),
        mix(from.green, to.green),
        mix(from.blue, to.blue),
        mix(from.alpha, to.alpha),
    )
}

/// A color with its RGB channels clamped to `[0, 1]`.
fn clamp_unit_color(color: LinearColor) -> LinearColor {
    LinearColor::new(color.red.clamp(0.0, 1.0), color.green.clamp(0.0, 1.0), color.blue.clamp(0.0, 1.0), color.alpha)
}

/// Rec. 709 luminance of a linear color's RGB.
fn luminance(color: LinearColor) -> f32 {
    (color.red * 0.2126) + (color.green * 0.7152) + (color.blue * 0.0722)
}

#[cfg(test)]
#[path = "material.test.rs"]
mod tests;
