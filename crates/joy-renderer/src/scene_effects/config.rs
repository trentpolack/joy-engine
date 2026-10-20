// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene effect options, defaults, and validation.
//!
//! Each effect is off (`None`) or on with its settings; `Some(Default::default())` enables an effect with its defaults.
//! [`SceneEffectsOptions::validate`] checks every setting's accepted range. Distances are world units, times seconds,
//! and screen lengths fractions of the shorter viewport axis.

use joy_core::LinearColor;
use serde::{
    Deserialize,
    Serialize,
};

/// Temporal antialiasing: the camera jitters by a sub-pixel offset every frame, and each pixel blends with its
/// reprojected history, clipped to the current neighborhood so moving content does not ghost.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct TaaOptions {
    /// Weight of the accumulated history each frame in `[0, 0.98]`; higher is smoother and slower to respond.
    pub history_weight: f32,
}

impl Default for TaaOptions {
    fn default() -> Self {
        Self {
            history_weight: 0.9,
        }
    }
}

/// Camera and object motion blur reconstructed from the scene's motion vectors.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct MotionBlurOptions {
    /// Fraction of each frame the shutter is open, in `[0, 1]`: one blurs across a whole frame's motion, one half is a
    /// film camera's 180-degree shutter.
    pub shutter: f32,
    /// Longest blur as a fraction of the shorter viewport axis, in `(0, 0.1]`.
    pub max_length: f32,
    /// Samples along each pixel's blur, 4 to 32.
    pub sample_count: u32,
}

impl Default for MotionBlurOptions {
    fn default() -> Self {
        Self {
            shutter: 0.5,
            max_length: 0.03,
            sample_count: 12,
        }
    }
}

/// Exposure that adapts to the scene's brightness, as an eye or camera does. The metered average maps to
/// `key_value`; the display chain's authored exposure multiplies the result.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AdaptiveExposureOptions {
    /// Exposed luminance the metered scene average maps to: middle gray.
    pub key_value: f32,
    /// Lowest exposure multiplier adaptation may reach.
    pub minimum_exposure: f32,
    /// Highest exposure multiplier adaptation may reach.
    pub maximum_exposure: f32,
    /// Exposure before the first frame is metered and after history resets.
    pub initial_exposure: f32,
    /// How quickly exposure rises toward its target when the scene darkens, per second.
    pub brighten_speed: f32,
    /// How quickly exposure falls toward its target when the scene brightens, per second.
    pub darken_speed: f32,
    /// Fraction of the darkest metered light the average ignores, so deep shadows do not swing exposure. The
    /// percentiles apply where compute shaders meter a histogram; the WebGL 2 meter averages everything.
    pub low_percentile: f32,
    /// Fraction of metered light below the brightest the average keeps, so small light sources do not swing exposure.
    pub high_percentile: f32,
    /// How much the center of the image dominates the meter, from 0 (evenly) to 1 (center only).
    pub center_weight: f32,
}

impl Default for AdaptiveExposureOptions {
    fn default() -> Self {
        Self {
            key_value: 0.18,
            minimum_exposure: 1.0 / 16.0,
            maximum_exposure: 16.0,
            initial_exposure: 1.0,
            brighten_speed: 1.0,
            darken_speed: 3.0,
            low_percentile: 0.1,
            high_percentile: 0.95,
            center_weight: 0.5,
        }
    }
}

/// Resolution of the volumetric froxel grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumetricQuality {
    /// 16-pixel cells and 48 depth slices.
    Low,
    /// 12-pixel cells and 64 depth slices.
    #[default]
    Medium,
    /// 8-pixel cells and 96 depth slices.
    High,
}

impl VolumetricQuality {
    /// Screen pixels per froxel column and depth slices.
    pub(crate) fn grid(self) -> (u32, u32) {
        match self {
            Self::Low => (16, 48),
            Self::Medium => (12, 64),
            Self::High => (8, 96),
        }
    }
}

/// Volumetric fog lit by the key light, point lights, and the skylight: light shafts, glows around lights, and
/// height fog in one participating medium. Fog applies to opaque surfaces, the sky, and transparency. Requires compute
/// shaders (not WebGL 2).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct VolumetricOptions {
    /// Extinction per world unit at the base height, in `[0, 1]`.
    pub density: f32,
    /// How quickly density falls off with height above the base, per world unit, in `[0, 10]`.
    pub height_falloff: f32,
    /// World height where density equals `density`.
    pub base_height: f32,
    /// Fraction of extinguished light the fog scatters, per channel in `[0, 1]`; its color in light.
    pub albedo: LinearColor,
    /// Light the fog emits on its own per world unit at the base density, linear RGB.
    pub emissive: LinearColor,
    /// Henyey-Greenstein anisotropy in `[-0.95, 0.95]`; positive values scatter forward, brightening fog toward
    /// lights in front of the viewer.
    pub anisotropy: f32,
    /// Multiplier on light the fog scatters from the key light, at least zero.
    pub directional_intensity: f32,
    /// Multiplier on light the fog scatters from point lights, at least zero.
    pub point_light_intensity: f32,
    /// Multiplier on light the fog scatters from the skylight and ambient light, at least zero.
    pub ambient_intensity: f32,
    /// Whether the key light and shadowed point lights cast volumetric shadows (light shafts).
    pub shadows: bool,
    /// Farthest view depth the volume covers; fog beyond it stays at its value there.
    pub max_distance: f32,
    /// Froxel grid resolution.
    pub quality: VolumetricQuality,
    /// Weight of the reprojected volume each frame in `[0, 0.98]`; higher hides noise and responds more slowly.
    pub history_weight: f32,
}

impl Default for VolumetricOptions {
    fn default() -> Self {
        Self {
            density: 0.02,
            height_falloff: 0.1,
            base_height: 0.0,
            albedo: LinearColor::rgb(0.9, 0.9, 0.9),
            emissive: LinearColor::BLACK,
            anisotropy: 0.5,
            directional_intensity: 1.0,
            point_light_intensity: 1.0,
            ambient_intensity: 1.0,
            shadows: true,
            max_distance: 100.0,
            quality: VolumetricQuality::Medium,
            history_weight: 0.9,
        }
    }
}

/// The scene effects a renderer runs; every field `None` runs none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SceneEffectsOptions {
    /// Temporal antialiasing.
    pub taa: Option<TaaOptions>,
    /// Motion blur.
    pub motion_blur: Option<MotionBlurOptions>,
    /// Adaptive exposure.
    pub adaptive_exposure: Option<AdaptiveExposureOptions>,
    /// Volumetric fog and lighting.
    pub volumetrics: Option<VolumetricOptions>,
}

/// Temporal antialiasing's name in its camelCase options spelling, as capabilities and errors report it.
pub const TAA_NAME: &str = "taa";
/// Motion blur's options name.
pub const MOTION_BLUR_NAME: &str = "motionBlur";
/// Adaptive exposure's options name.
pub const ADAPTIVE_EXPOSURE_NAME: &str = "adaptiveExposure";
/// Volumetric fog and lighting's options name.
pub const VOLUMETRICS_NAME: &str = "volumetrics";

/// Every effect's camelCase name, in display order.
pub const SCENE_EFFECT_NAMES: [&str; 4] = [TAA_NAME, MOTION_BLUR_NAME, ADAPTIVE_EXPOSURE_NAME, VOLUMETRICS_NAME];

/// A scene effect setting outside its accepted range.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SceneEffectsError {
    /// One setting is out of range or not finite.
    #[error("Scene effect {effect}.{setting} must be {expected}.")]
    OutOfRange {
        /// The effect's camelCase name.
        effect: &'static str,
        /// The setting's camelCase name.
        setting: &'static str,
        /// The accepted range, for the message.
        expected: &'static str,
    },
}

impl SceneEffectsOptions {
    /// Whether any effect is enabled.
    pub fn any_enabled(&self) -> bool {
        self.enabled_names().next().is_some()
    }

    /// The enabled effects' names, in display order.
    pub fn enabled_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        let enabled = [
            self.taa.is_some(),
            self.motion_blur.is_some(),
            self.adaptive_exposure.is_some(),
            self.volumetrics.is_some(),
        ];
        SCENE_EFFECT_NAMES.into_iter().zip(enabled).filter_map(|(name, on)| on.then_some(name))
    }

    /// Whether the scene pass must write motion vectors (temporal antialiasing and motion blur read them).
    pub fn needs_motion_vectors(&self) -> bool {
        self.taa.is_some() || self.motion_blur.is_some()
    }

    /// Check every enabled effect's settings.
    pub fn validate(&self) -> Result<(), SceneEffectsError> {
        if let Some(taa) = &self.taa {
            check(TAA_NAME, "historyWeight", taa.history_weight, 0.0, 0.98, "in [0, 0.98]")?;
        }
        if let Some(blur) = &self.motion_blur {
            check(MOTION_BLUR_NAME, "shutter", blur.shutter, 0.0, 1.0, "in [0, 1]")?;
            check_positive(MOTION_BLUR_NAME, "maxLength", blur.max_length, 0.1, "in (0, 0.1]")?;
            if !(4..=32).contains(&blur.sample_count) {
                return Err(out_of_range(MOTION_BLUR_NAME, "sampleCount", "4 to 32"));
            }
        }
        if let Some(exposure) = &self.adaptive_exposure {
            const RANGE: &str = "positive and finite";
            check_positive(ADAPTIVE_EXPOSURE_NAME, "keyValue", exposure.key_value, f32::MAX, RANGE)?;
            check_positive(ADAPTIVE_EXPOSURE_NAME, "minimumExposure", exposure.minimum_exposure, f32::MAX, RANGE)?;
            check_positive(ADAPTIVE_EXPOSURE_NAME, "maximumExposure", exposure.maximum_exposure, f32::MAX, RANGE)?;
            if exposure.maximum_exposure < exposure.minimum_exposure {
                return Err(out_of_range(ADAPTIVE_EXPOSURE_NAME, "maximumExposure", "at least minimumExposure"));
            }
            check_positive(ADAPTIVE_EXPOSURE_NAME, "initialExposure", exposure.initial_exposure, f32::MAX, RANGE)?;
            check(ADAPTIVE_EXPOSURE_NAME, "brightenSpeed", exposure.brighten_speed, 0.0, 100.0, "in [0, 100]")?;
            check(ADAPTIVE_EXPOSURE_NAME, "darkenSpeed", exposure.darken_speed, 0.0, 100.0, "in [0, 100]")?;
            check(ADAPTIVE_EXPOSURE_NAME, "lowPercentile", exposure.low_percentile, 0.0, 0.99, "in [0, 0.99]")?;
            check(ADAPTIVE_EXPOSURE_NAME, "highPercentile", exposure.high_percentile, 0.01, 1.0, "in [0.01, 1]")?;
            if exposure.high_percentile <= exposure.low_percentile {
                return Err(out_of_range(ADAPTIVE_EXPOSURE_NAME, "highPercentile", "above lowPercentile"));
            }
            check(ADAPTIVE_EXPOSURE_NAME, "centerWeight", exposure.center_weight, 0.0, 1.0, "in [0, 1]")?;
        }
        if let Some(volumetrics) = &self.volumetrics {
            check(VOLUMETRICS_NAME, "density", volumetrics.density, 0.0, 1.0, "in [0, 1]")?;
            check(VOLUMETRICS_NAME, "heightFalloff", volumetrics.height_falloff, 0.0, 10.0, "in [0, 10]")?;
            check(VOLUMETRICS_NAME, "baseHeight", volumetrics.base_height, -1.0e6, 1.0e6, "finite")?;
            for channel in volumetrics.albedo.rgb_array() {
                check(VOLUMETRICS_NAME, "albedo", channel, 0.0, 1.0, "in [0, 1] per channel")?;
            }
            for channel in volumetrics.emissive.rgb_array() {
                check(VOLUMETRICS_NAME, "emissive", channel, 0.0, f32::MAX, "non-negative")?;
            }
            check(VOLUMETRICS_NAME, "anisotropy", volumetrics.anisotropy, -0.95, 0.95, "in [-0.95, 0.95]")?;
            for (setting, value) in [
                ("directionalIntensity", volumetrics.directional_intensity),
                ("pointLightIntensity", volumetrics.point_light_intensity),
                ("ambientIntensity", volumetrics.ambient_intensity),
            ] {
                check(VOLUMETRICS_NAME, setting, value, 0.0, f32::MAX, "non-negative")?;
            }
            check_positive(VOLUMETRICS_NAME, "maxDistance", volumetrics.max_distance, 1.0e5, "in (0, 100000]")?;
            check(VOLUMETRICS_NAME, "historyWeight", volumetrics.history_weight, 0.0, 0.98, "in [0, 0.98]")?;
        }
        Ok(())
    }
}

fn out_of_range(effect: &'static str, setting: &'static str, expected: &'static str) -> SceneEffectsError {
    SceneEffectsError::OutOfRange {
        effect,
        setting,
        expected,
    }
}

/// Reject a value outside `[minimum, maximum]` (NaN included).
fn check(
    effect: &'static str,
    setting: &'static str,
    value: f32,
    minimum: f32,
    maximum: f32,
    expected: &'static str,
) -> Result<(), SceneEffectsError> {
    if (minimum..=maximum).contains(&value) { Ok(()) } else { Err(out_of_range(effect, setting, expected)) }
}

/// Reject a value outside `(0, maximum]` (NaN included).
fn check_positive(
    effect: &'static str,
    setting: &'static str,
    value: f32,
    maximum: f32,
    expected: &'static str,
) -> Result<(), SceneEffectsError> {
    if value > 0.0 && value <= maximum { Ok(()) } else { Err(out_of_range(effect, setting, expected)) }
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
