// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Sky settings the renderer reads every frame: atmospheric scattering and fog, the procedural cloud layer, light
//! shafts, and ambient occlusion. Presentation only; the sun, moon, skylight, and wind that light the sky come from the
//! environment simulation (`joy-environment`), and `joy-scene` combines both into [`EnvironmentLighting`].
//!
//! Serialized names, defaults, validation ranges, and messages match the TypeScript engine's environment options, so
//! authored values written for those options read the same way here.
//!
//! [`EnvironmentLighting`]: crate::EnvironmentLighting

use joy_core::format::format_number;
use serde::{
    Deserialize,
    Serialize,
};

/// The smallest positive `f64`; strictly positive ranges start here, matching the TypeScript `Number.MIN_VALUE`.
const MIN_POSITIVE_VALUE: f64 = 5e-324;

/// Atmospheric scattering scales. All values are nonnegative multipliers except `fog_density` (per meter).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AtmosphereOptions {
    /// Rayleigh (sky blue) scattering multiplier.
    pub rayleigh: f64,
    /// Mie (haze) scattering multiplier.
    pub mie: f64,
    /// Ozone absorption multiplier.
    pub ozone: f64,
    /// Overall atmosphere density multiplier.
    pub density: f64,
    /// Exponential height-fog density per meter.
    pub fog_density: f64,
}

impl Default for AtmosphereOptions {
    fn default() -> Self {
        Self { rayleigh: 1.0, mie: 1.0, ozone: 1.0, density: 1.0, fog_density: 0.002 }
    }
}

/// Cloud layer policy. Fractions are in `[0, 1]`; altitude and thickness are meters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct CloudOptions {
    /// Sky fraction covered by clouds, in `[0, 1]`.
    pub coverage: f64,
    /// Cloud optical density, in `[0, 1]`.
    pub density: f64,
    /// Cloud base altitude in meters; nonnegative.
    pub altitude: f64,
    /// Cloud layer thickness in meters; strictly positive.
    pub thickness: f64,
    /// Cloud shadow strength on the ground, in `[0, 1]`.
    pub shadow_strength: f64,
}

impl Default for CloudOptions {
    fn default() -> Self {
        Self { coverage: 0.45, density: 0.7, altitude: 1800.0, thickness: 700.0, shadow_strength: 0.45 }
    }
}

/// Screen-space light shaft policy.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct LightShaftOptions {
    /// Shaft intensity; nonnegative.
    pub intensity: f64,
    /// Per-sample decay, in `[0, 1]`.
    pub decay: f64,
    /// Shaft exposure; nonnegative.
    pub exposure: f64,
}

impl Default for LightShaftOptions {
    fn default() -> Self {
        Self { intensity: 0.65, decay: 0.94, exposure: 0.2 }
    }
}

/// Ambient occlusion policy.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AmbientOcclusionOptions {
    /// Occlusion intensity; nonnegative.
    pub intensity: f64,
    /// Occlusion radius in meters; strictly positive.
    pub radius: f64,
}

impl Default for AmbientOcclusionOptions {
    fn default() -> Self {
        Self { intensity: 1.0, radius: 7.0 }
    }
}

/// Every sky setting the renderer reads. Omitted fields (including fields of nested records) take their defaults, and
/// unknown fields are rejected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SkyOptions {
    /// Atmosphere scattering and fog.
    pub atmosphere: AtmosphereOptions,
    /// The procedural cloud layer.
    pub clouds: CloudOptions,
    /// Screen-space light shafts.
    pub light_shafts: LightShaftOptions,
    /// Screen-space ambient occlusion.
    pub ambient_occlusion: AmbientOcclusionOptions,
}

impl SkyOptions {
    /// Read settings from an authored JSON value. Omitted fields keep their defaults; unknown fields, wrong types, and
    /// out-of-range values are rejected. The input is never retained.
    pub fn from_json(value: &serde_json::Value) -> Result<Self, SkyOptionsError> {
        let options: Self = serde_json::from_value(value.clone())
            .map_err(|error| SkyOptionsError::InvalidOptions(error.to_string()))?;
        options.validate()?;
        Ok(options)
    }

    /// Check every setting against the TypeScript engine's ranges, in the same order.
    pub fn validate(&self) -> Result<(), SkyOptionsError> {
        let atmosphere = &self.atmosphere;
        check_range(atmosphere.rayleigh, 0.0, f64::INFINITY, "atmosphere.rayleigh")?;
        check_range(atmosphere.mie, 0.0, f64::INFINITY, "atmosphere.mie")?;
        check_range(atmosphere.ozone, 0.0, f64::INFINITY, "atmosphere.ozone")?;
        check_range(atmosphere.density, 0.0, f64::INFINITY, "atmosphere.density")?;
        check_range(atmosphere.fog_density, 0.0, f64::INFINITY, "atmosphere.fogDensity")?;

        check_range(self.clouds.coverage, 0.0, 1.0, "clouds.coverage")?;
        check_range(self.clouds.density, 0.0, 1.0, "clouds.density")?;
        check_range(self.clouds.shadow_strength, 0.0, 1.0, "clouds.shadowStrength")?;
        check_range(self.clouds.altitude, 0.0, f64::INFINITY, "clouds.altitude")?;
        check_range(self.clouds.thickness, MIN_POSITIVE_VALUE, f64::INFINITY, "clouds.thickness")?;

        check_range(self.light_shafts.intensity, 0.0, f64::INFINITY, "lightShafts.intensity")?;
        check_range(self.light_shafts.decay, 0.0, 1.0, "lightShafts.decay")?;
        check_range(self.light_shafts.exposure, 0.0, f64::INFINITY, "lightShafts.exposure")?;
        check_range(self.ambient_occlusion.intensity, 0.0, f64::INFINITY, "ambientOcclusion.intensity")?;
        check_range(self.ambient_occlusion.radius, MIN_POSITIVE_VALUE, f64::INFINITY, "ambientOcclusion.radius")?;
        Ok(())
    }
}

/// Sky settings validation failure. Messages match the TypeScript engine.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SkyOptionsError {
    /// A value was non-finite or outside its inclusive range.
    #[error("{name} must be finite and between {} and {}.", format_number(*.minimum), format_number(*.maximum))]
    OutOfRange {
        /// Serialized setting name.
        name: &'static str,
        /// Inclusive lower bound.
        minimum: f64,
        /// Inclusive upper bound.
        maximum: f64,
    },
    /// An authored settings record had unknown fields or wrong value types.
    #[error("Invalid sky options: {0}")]
    InvalidOptions(String),
}

fn check_range(value: f64, minimum: f64, maximum: f64, name: &'static str) -> Result<(), SkyOptionsError> {
    if !value.is_finite() || value < minimum || value > maximum {
        return Err(SkyOptionsError::OutOfRange { name, minimum, maximum });
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/sky.rs"]
mod tests;
