// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene effect options, defaults, and validation.
//!
//! Each effect is off (`None`) or on with its settings. `Some(Default::default())` enables an effect with its default
//! settings. [`SceneEffectsOptions::validate`] checks every setting's accepted range and relationships.

use joy_core::LinearColor;
use serde::{
    Deserialize,
    Serialize,
};

/// Where depth-based effects read surface normals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NormalSource {
    /// Reconstruct normals from neighboring depth samples.
    #[default]
    ReconstructFromDepth,
    /// Read the renderer's view-space normal attachment.
    NormalTexture,
}

/// How temporal antialiasing finds last frame's pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reprojection {
    /// Per-pixel motion vectors from the motion attachment (moving meshes reproject correctly).
    #[default]
    Velocity,
    /// Camera matrices only (static scenes).
    Camera,
}

/// Intermediate color format of the effect chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SceneEffectsColorFormat {
    /// Half-float HDR color.
    Rgba16float,
    /// 8-bit color, for devices without renderable, filterable, blendable half floats.
    Rgba8unorm,
}

/// Depth-aware separable blur of the whole scene.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct DepthAwareBlurOptions {
    /// Taps per side, 1 to 8.
    pub radius: f32,
    /// Depth difference falloff.
    pub depth_sigma: f32,
    /// Spatial falloff in taps.
    pub spatial_sigma: f32,
}

impl Default for DepthAwareBlurOptions {
    fn default() -> Self {
        Self { radius: 4.0, depth_sigma: 0.01, spatial_sigma: 3.0 }
    }
}

/// Screen-space ambient occlusion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SsaoOptions {
    /// Sample radius in pixels.
    pub radius: f32,
    /// Depth bias in world units.
    pub bias: f32,
    /// Occlusion strength.
    pub intensity: f32,
    /// Intermediate resolution, 0.25 to 1.
    pub resolution_scale: f32,
    /// Normal source.
    pub normal_source: NormalSource,
}

impl Default for SsaoOptions {
    fn default() -> Self {
        Self { radius: 7.0, bias: 0.03, intensity: 1.35, resolution_scale: 1.0, normal_source: NormalSource::default() }
    }
}

/// Ground-truth ambient occlusion with temporal accumulation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct GtaoOptions {
    /// World-space radius.
    pub radius: f32,
    /// Horizon bias.
    pub bias: f32,
    /// Occlusion strength.
    pub intensity: f32,
    /// Intermediate resolution, 0.25 to 1.
    pub resolution_scale: f32,
    /// Weight of accumulated history, up to 0.98.
    pub history_weight: f32,
    /// History rejection depth threshold.
    pub depth_threshold: f32,
    /// Composite strength.
    pub strength: f32,
}

impl Default for GtaoOptions {
    fn default() -> Self {
        Self {
            radius: 2.2,
            bias: 0.04,
            intensity: 3.2,
            resolution_scale: 1.0,
            history_weight: 0.85,
            depth_threshold: 0.02,
            strength: 1.0,
        }
    }
}

/// Screen-space global illumination.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SsgiOptions {
    /// World-space trace radius.
    pub radius: f32,
    /// Assumed surface thickness.
    pub thickness: f32,
    /// Indirect light strength.
    pub intensity: f32,
    /// Rays per pixel, 1 to 12.
    pub ray_count: f32,
    /// Steps per ray, 2 to 12.
    pub step_count: f32,
    /// Intermediate resolution, 0.25 to 1.
    pub resolution_scale: f32,
    /// Weight of accumulated history, up to 0.98.
    pub history_weight: f32,
    /// History rejection depth threshold.
    pub depth_threshold: f32,
    /// Composite strength.
    pub strength: f32,
}

impl Default for SsgiOptions {
    fn default() -> Self {
        Self {
            radius: 4.5,
            thickness: 0.32,
            intensity: 2.2,
            ray_count: 7.0,
            step_count: 8.0,
            resolution_scale: 0.5,
            history_weight: 0.85,
            depth_threshold: 0.02,
            strength: 1.0,
        }
    }
}

/// Depth and normal edge outlines.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct OutlineOptions {
    /// Linear outline color; alpha scales coverage.
    pub color: LinearColor,
    /// Line width in pixels.
    pub thickness: f32,
    /// Depth edge threshold.
    pub depth_threshold: f32,
    /// Normal edge threshold.
    pub normal_threshold: f32,
    /// Normal source.
    pub normal_source: NormalSource,
}

impl Default for OutlineOptions {
    fn default() -> Self {
        Self {
            color: LinearColor::rgb(0.02, 0.025, 0.035),
            thickness: 1.0,
            depth_threshold: 0.01,
            normal_threshold: 0.2,
            normal_source: NormalSource::default(),
        }
    }
}

/// Camera-motion blur from the motion attachment.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct MotionBlurOptions {
    /// Blur length multiplier.
    pub strength: f32,
    /// Samples along the motion vector, 2 to 16.
    pub sample_count: f32,
}

impl Default for MotionBlurOptions {
    fn default() -> Self {
        Self { strength: 1.0, sample_count: 10.0 }
    }
}

/// Temporal antialiasing with sub-pixel camera jitter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct TaaOptions {
    /// Weight of accumulated history, up to 0.98.
    pub history_weight: f32,
    /// History rejection depth threshold.
    pub depth_threshold: f32,
    /// Reprojection source.
    pub reprojection: Reprojection,
}

impl Default for TaaOptions {
    fn default() -> Self {
        Self { history_weight: 0.9, depth_threshold: 0.01, reprojection: Reprojection::default() }
    }
}

/// Screen-space reflections.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SsrOptions {
    /// Reflection strength.
    pub intensity: f32,
    /// Maximum ray length in world units, 1 to 1000.
    pub max_distance: f32,
    /// Assumed surface thickness.
    pub thickness: f32,
    /// Ray march samples, 8 to 96.
    pub sample_count: f32,
    /// Roughness above which reflections fade out, up to 1.
    pub max_roughness: f32,
    /// Intermediate resolution, 0.25 to 1.
    pub resolution_scale: f32,
    /// Weight of accumulated history, up to 0.98.
    pub history_weight: f32,
    /// History rejection depth threshold.
    pub depth_threshold: f32,
    /// Composite strength.
    pub strength: f32,
}

impl Default for SsrOptions {
    fn default() -> Self {
        Self {
            intensity: 1.35,
            max_distance: 60.0,
            thickness: 0.45,
            sample_count: 48.0,
            max_roughness: 0.88,
            resolution_scale: 0.5,
            history_weight: 0.85,
            depth_threshold: 0.02,
            strength: 1.0,
        }
    }
}

/// Exponential height fog.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct HeightFogOptions {
    /// Linear fog color; alpha scales the fog's opacity.
    pub fog_color: LinearColor,
    /// Fog density.
    pub density: f32,
    /// Density falloff with height.
    pub height_falloff: f32,
    /// In-scattering amount.
    pub scattering: f32,
}

impl Default for HeightFogOptions {
    fn default() -> Self {
        Self {
            fog_color: LinearColor::new(0.18, 0.34, 0.48, 0.6),
            density: 0.22,
            height_falloff: 3.0,
            scattering: 0.35,
        }
    }
}

/// Clustered volumetric lighting from the sun and point lights.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ClusteredLightingOptions {
    /// Intermediate resolution, 0.25 to 1.
    pub resolution_scale: f32,
    /// Linear fog color, stored as `[red, green, blue]`; alpha is unused.
    #[serde(with = "joy_core::color::rgb_array")]
    pub fog_color: LinearColor,
    /// Medium density.
    pub density: f32,
    /// Density falloff with height.
    pub height_falloff: f32,
    /// Fog layer height in world units.
    pub fog_height: f32,
    /// Henyey-Greenstein anisotropy, -0.8 to 0.85.
    pub anisotropy: f32,
    /// Sun contribution.
    pub directional_intensity: f32,
    /// Point light contribution.
    pub point_light_intensity: f32,
    /// March distance in world units, 1 to 1000.
    pub max_distance: f32,
    /// March samples, 3 to 20.
    pub sample_count: f32,
    /// Depth-shadow strength, up to 1.
    pub shadow_strength: f32,
    /// Weight of accumulated history, up to 0.98.
    pub history_weight: f32,
    /// History rejection depth threshold.
    pub depth_threshold: f32,
    /// Composite strength.
    pub strength: f32,
}

impl Default for ClusteredLightingOptions {
    fn default() -> Self {
        Self {
            resolution_scale: 0.5,
            fog_color: LinearColor::rgb(0.18, 0.3, 0.48),
            density: 0.055,
            height_falloff: 0.23,
            fog_height: 0.2,
            anisotropy: 0.45,
            directional_intensity: 2.2,
            point_light_intensity: 1.8,
            max_distance: 28.0,
            sample_count: 10.0,
            shadow_strength: 0.74,
            history_weight: 0.85,
            depth_threshold: 0.02,
            strength: 1.0,
        }
    }
}

/// Automatic HDR exposure from scene luminance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AdaptiveExposureOptions {
    /// First metering level's resolution, 0.25 to 1.
    pub metering_scale: f32,
    /// Exposure before adaptation.
    pub initial_exposure: f32,
    /// Target average luminance.
    pub key_value: f32,
    /// Lowest exposure.
    pub minimum_exposure: f32,
    /// Highest exposure.
    pub maximum_exposure: f32,
    /// Adaptation rate toward brighter exposure, per second.
    pub brighten_speed: f32,
    /// Adaptation rate toward darker exposure, per second.
    pub darken_speed: f32,
}

impl Default for AdaptiveExposureOptions {
    fn default() -> Self {
        Self {
            metering_scale: 0.25,
            initial_exposure: 1.0,
            key_value: 0.18,
            minimum_exposure: 0.125,
            maximum_exposure: 8.0,
            brighten_speed: 1.5,
            darken_speed: 3.0,
        }
    }
}

/// Opaque-scene effects that run after the opaque pass and before transparency and display processing.
///
/// Like TypeScript, effects need a WebGPU-class backend (native or browser WebGPU); WebGL 2 reports them unavailable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SceneEffectsOptions {
    /// Chain color format; defaults to half floats when the device supports them.
    pub color_format: Option<SceneEffectsColorFormat>,
    /// Depth-aware scene blur.
    pub depth_aware_blur: Option<DepthAwareBlurOptions>,
    /// Screen-space ambient occlusion (exclusive with `gtao`).
    pub ssao: Option<SsaoOptions>,
    /// Ground-truth ambient occlusion (exclusive with `ssao`).
    pub gtao: Option<GtaoOptions>,
    /// Screen-space global illumination.
    pub ssgi: Option<SsgiOptions>,
    /// Edge outlines.
    pub outlines: Option<OutlineOptions>,
    /// Temporal antialiasing.
    pub taa: Option<TaaOptions>,
    /// Motion blur.
    pub motion_blur: Option<MotionBlurOptions>,
    /// Screen-space reflections.
    pub ssr: Option<SsrOptions>,
    /// Height fog.
    pub height_fog: Option<HeightFogOptions>,
    /// Clustered volumetric lighting.
    pub clustered_lighting: Option<ClusteredLightingOptions>,
    /// Adaptive exposure.
    pub adaptive_exposure: Option<AdaptiveExposureOptions>,
}

/// Effect names in TypeScript order, for capability reports and controls.
pub const SCENE_EFFECT_NAMES: [&str; 11] = [
    "depthAwareBlur",
    "ssao",
    "gtao",
    "ssgi",
    "outlines",
    "taa",
    "motionBlur",
    "ssr",
    "heightFog",
    "clusteredLighting",
    "adaptiveExposure",
];

/// Attachments and history the enabled effects need from the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneEffectsRequirements {
    /// Scene depth.
    pub depth_texture: bool,
    /// View-space normal and roughness attachment.
    pub normal_texture: bool,
    /// Jittered motion vectors for lighting effects.
    pub velocity_texture: bool,
    /// Unjittered motion vectors for antialiasing and motion blur.
    pub taa_velocity_texture: bool,
    /// Full-precision depth history.
    pub precise_depth_history: bool,
    /// Camera matrices.
    pub camera_matrices: bool,
    /// Clustered point-light buffers.
    pub clustered_lighting: bool,
}

/// Invalid scene effect options or frame inputs. Messages follow the TypeScript engine.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SceneEffectsError {
    /// A setting was outside its range.
    #[error("{name} must be a finite number from {minimum} through {maximum}.")]
    OutOfRange {
        /// `effect.setting`.
        name: String,
        /// Inclusive minimum.
        minimum: String,
        /// Inclusive maximum.
        maximum: String,
    },
    /// A count setting was fractional.
    #[error("{0} must be an integer.")]
    NotAnInteger(String),
    /// Both ambient-occlusion implementations were enabled.
    #[error("ssao and gtao are mutually exclusive ambient-occlusion implementations.")]
    ExclusiveOcclusion,
    /// Adaptive exposure bounds were reversed.
    #[error("adaptiveExposure.minimumExposure must not exceed maximumExposure.")]
    ExposureBounds,
    /// The device cannot render the requested chain format.
    #[error("Scene effects colorFormat rgba16float is not supported by this device.")]
    UnsupportedColorFormat,
    /// An effect could not be used with the current frame.
    #[error("{0}")]
    InvalidFrame(String),
}

/// Inclusive range check matching the TypeScript `assertRange`.
fn check(name: &str, key: &str, value: f32, minimum: f32, maximum: f32) -> Result<(), SceneEffectsError> {
    if value.is_finite() && value >= minimum && value <= maximum {
        return Ok(());
    }
    let format = |bound: f32| {
        if bound == f32::INFINITY {
            "Infinity".to_owned()
        } else if bound == f32::NEG_INFINITY {
            "-Infinity".to_owned()
        } else if bound == POSITIVE {
            "5e-324".to_owned()
        } else {
            format!("{bound}")
        }
    };
    Err(SceneEffectsError::OutOfRange {
        name: format!("{name}.{key}"),
        minimum: format(minimum),
        maximum: format(maximum),
    })
}

/// Integer check for counts.
fn integer(name: &str, key: &str, value: f32) -> Result<(), SceneEffectsError> {
    if value.fract() == 0.0 { Ok(()) } else { Err(SceneEffectsError::NotAnInteger(format!("{name}.{key}"))) }
}

/// Positive-only bound: the smallest positive value, standing in for TypeScript's `Number.MIN_VALUE`.
const POSITIVE: f32 = f32::from_bits(1);
const UNBOUNDED: f32 = f32::INFINITY;

fn check_color(name: &str, key: &str, color: &[f32]) -> Result<(), SceneEffectsError> {
    color.iter().try_for_each(|channel| check(name, key, *channel, 0.0, 1.0))
}

impl SceneEffectsOptions {
    /// Whether any effect is enabled.
    pub fn any_enabled(&self) -> bool {
        self.enabled_names().next().is_some()
    }

    /// Names of the enabled effects in TypeScript order.
    pub fn enabled_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        let enabled = [
            self.depth_aware_blur.is_some(),
            self.ssao.is_some(),
            self.gtao.is_some(),
            self.ssgi.is_some(),
            self.outlines.is_some(),
            self.taa.is_some(),
            self.motion_blur.is_some(),
            self.ssr.is_some(),
            self.height_fog.is_some(),
            self.clustered_lighting.is_some(),
            self.adaptive_exposure.is_some(),
        ];
        SCENE_EFFECT_NAMES.into_iter().zip(enabled).filter_map(|(name, on)| on.then_some(name))
    }

    /// Check every setting against the TypeScript ranges and rules.
    pub fn validate(&self) -> Result<(), SceneEffectsError> {
        if let Some(blur) = &self.depth_aware_blur {
            let name = "depthAwareBlur";
            check(name, "radius", blur.radius, 1.0, 8.0)?;
            integer(name, "radius", blur.radius)?;
            check(name, "depthSigma", blur.depth_sigma, POSITIVE, UNBOUNDED)?;
            check(name, "spatialSigma", blur.spatial_sigma, POSITIVE, UNBOUNDED)?;
        }
        if let Some(ssao) = &self.ssao {
            let name = "ssao";
            check(name, "radius", ssao.radius, 1.0, UNBOUNDED)?;
            check(name, "bias", ssao.bias, 0.0, UNBOUNDED)?;
            check(name, "intensity", ssao.intensity, 0.0, UNBOUNDED)?;
            check(name, "resolutionScale", ssao.resolution_scale, 0.25, 1.0)?;
        }
        if let Some(gtao) = &self.gtao {
            let name = "gtao";
            check(name, "radius", gtao.radius, 0.1, UNBOUNDED)?;
            check(name, "bias", gtao.bias, 0.0, UNBOUNDED)?;
            check(name, "intensity", gtao.intensity, 0.0, UNBOUNDED)?;
            check(name, "resolutionScale", gtao.resolution_scale, 0.25, 1.0)?;
            check(name, "historyWeight", gtao.history_weight, 0.0, 0.98)?;
            check(name, "depthThreshold", gtao.depth_threshold, POSITIVE, UNBOUNDED)?;
            check(name, "strength", gtao.strength, 0.0, UNBOUNDED)?;
        }
        if let Some(ssgi) = &self.ssgi {
            let name = "ssgi";
            check(name, "radius", ssgi.radius, 0.1, UNBOUNDED)?;
            check(name, "thickness", ssgi.thickness, POSITIVE, UNBOUNDED)?;
            check(name, "intensity", ssgi.intensity, 0.0, UNBOUNDED)?;
            check(name, "rayCount", ssgi.ray_count, 1.0, 12.0)?;
            integer(name, "rayCount", ssgi.ray_count)?;
            check(name, "stepCount", ssgi.step_count, 2.0, 12.0)?;
            integer(name, "stepCount", ssgi.step_count)?;
            check(name, "resolutionScale", ssgi.resolution_scale, 0.25, 1.0)?;
            check(name, "historyWeight", ssgi.history_weight, 0.0, 0.98)?;
            check(name, "depthThreshold", ssgi.depth_threshold, POSITIVE, UNBOUNDED)?;
            check(name, "strength", ssgi.strength, 0.0, UNBOUNDED)?;
        }
        if let Some(outlines) = &self.outlines {
            let name = "outlines";
            check_color(name, "color", &outlines.color.to_array())?;
            check(name, "thickness", outlines.thickness, POSITIVE, UNBOUNDED)?;
            check(name, "depthThreshold", outlines.depth_threshold, POSITIVE, UNBOUNDED)?;
            check(name, "normalThreshold", outlines.normal_threshold, 0.0, UNBOUNDED)?;
        }
        if let Some(taa) = &self.taa {
            check("taa", "historyWeight", taa.history_weight, 0.0, 0.98)?;
            check("taa", "depthThreshold", taa.depth_threshold, POSITIVE, UNBOUNDED)?;
        }
        if let Some(blur) = &self.motion_blur {
            check("motionBlur", "strength", blur.strength, 0.0, UNBOUNDED)?;
            check("motionBlur", "sampleCount", blur.sample_count, 2.0, 16.0)?;
            integer("motionBlur", "sampleCount", blur.sample_count)?;
        }
        if let Some(ssr) = &self.ssr {
            let name = "ssr";
            check(name, "intensity", ssr.intensity, 0.0, UNBOUNDED)?;
            check(name, "maxDistance", ssr.max_distance, 1.0, 1000.0)?;
            check(name, "thickness", ssr.thickness, POSITIVE, UNBOUNDED)?;
            check(name, "sampleCount", ssr.sample_count, 8.0, 96.0)?;
            integer(name, "sampleCount", ssr.sample_count)?;
            check(name, "maxRoughness", ssr.max_roughness, 0.0, 1.0)?;
            check(name, "resolutionScale", ssr.resolution_scale, 0.25, 1.0)?;
            check(name, "historyWeight", ssr.history_weight, 0.0, 0.98)?;
            check(name, "depthThreshold", ssr.depth_threshold, POSITIVE, UNBOUNDED)?;
            check(name, "strength", ssr.strength, 0.0, UNBOUNDED)?;
        }
        if let Some(fog) = &self.height_fog {
            let name = "heightFog";
            check_color(name, "fogColor", &fog.fog_color.to_array())?;
            check(name, "density", fog.density, 0.0, UNBOUNDED)?;
            check(name, "heightFalloff", fog.height_falloff, 0.0, UNBOUNDED)?;
            check(name, "scattering", fog.scattering, 0.0, UNBOUNDED)?;
        }
        if let Some(clustered) = &self.clustered_lighting {
            let name = "clusteredLighting";
            check(name, "resolutionScale", clustered.resolution_scale, 0.25, 1.0)?;
            check_color(name, "fogColor", &clustered.fog_color.rgb_array())?;
            check(name, "density", clustered.density, 0.0, UNBOUNDED)?;
            check(name, "heightFalloff", clustered.height_falloff, 0.0, UNBOUNDED)?;
            check(name, "fogHeight", clustered.fog_height, f32::NEG_INFINITY, UNBOUNDED)?;
            check(name, "anisotropy", clustered.anisotropy, -0.8, 0.85)?;
            check(name, "directionalIntensity", clustered.directional_intensity, 0.0, UNBOUNDED)?;
            check(name, "pointLightIntensity", clustered.point_light_intensity, 0.0, UNBOUNDED)?;
            check(name, "maxDistance", clustered.max_distance, 1.0, 1000.0)?;
            check(name, "sampleCount", clustered.sample_count, 3.0, 20.0)?;
            integer(name, "sampleCount", clustered.sample_count)?;
            check(name, "shadowStrength", clustered.shadow_strength, 0.0, 1.0)?;
            check(name, "historyWeight", clustered.history_weight, 0.0, 0.98)?;
            check(name, "depthThreshold", clustered.depth_threshold, POSITIVE, UNBOUNDED)?;
            check(name, "strength", clustered.strength, 0.0, UNBOUNDED)?;
        }
        if let Some(exposure) = &self.adaptive_exposure {
            let name = "adaptiveExposure";
            check(name, "meteringScale", exposure.metering_scale, 0.25, 1.0)?;
            check(name, "initialExposure", exposure.initial_exposure, POSITIVE, UNBOUNDED)?;
            check(name, "keyValue", exposure.key_value, POSITIVE, UNBOUNDED)?;
            check(name, "minimumExposure", exposure.minimum_exposure, POSITIVE, UNBOUNDED)?;
            check(name, "maximumExposure", exposure.maximum_exposure, POSITIVE, UNBOUNDED)?;
            check(name, "brightenSpeed", exposure.brighten_speed, 0.0, UNBOUNDED)?;
            check(name, "darkenSpeed", exposure.darken_speed, 0.0, UNBOUNDED)?;
        }
        if self.ssao.is_some() && self.gtao.is_some() {
            return Err(SceneEffectsError::ExclusiveOcclusion);
        }
        if let Some(exposure) = &self.adaptive_exposure
            && exposure.minimum_exposure > exposure.maximum_exposure
        {
            return Err(SceneEffectsError::ExposureBounds);
        }
        Ok(())
    }

    /// What the enabled effects need from the renderer.
    pub fn requirements(&self) -> SceneEffectsRequirements {
        let normal_texture = self.gtao.is_some()
            || self.ssgi.is_some()
            || self.ssr.is_some()
            || self.ssao.is_some_and(|ssao| ssao.normal_source == NormalSource::NormalTexture)
            || self.outlines.is_some_and(|outlines| outlines.normal_source == NormalSource::NormalTexture);
        let depth_texture = self.enabled_names().any(|name| name != "adaptiveExposure");
        SceneEffectsRequirements {
            depth_texture,
            normal_texture,
            velocity_texture: self.gtao.is_some()
                || self.ssgi.is_some()
                || self.ssr.is_some()
                || self.clustered_lighting.is_some(),
            taa_velocity_texture: self.motion_blur.is_some()
                || self.taa.is_some_and(|taa| taa.reprojection == Reprojection::Velocity),
            precise_depth_history: self.gtao.is_some()
                || self.ssgi.is_some()
                || self.ssr.is_some()
                || self.taa.is_some(),
            camera_matrices: self.gtao.is_some()
                || self.ssgi.is_some()
                || self.ssr.is_some()
                || self.clustered_lighting.is_some()
                || self.taa.is_some_and(|taa| taa.reprojection == Reprojection::Camera),
            clustered_lighting: self.clustered_lighting.is_some(),
        }
    }
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
