// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Deterministic world environment: a solar clock, celestial lights, skylight, and a gusting wind field.
//!
//! This is a port of the TypeScript `EnvironmentSystem`. Options, defaults, validation ranges, and the frame math are
//! identical; every quantity is computed in `f64` in the same order as the TypeScript source and only then stored as
//! `f32` in the frame. Time values are seconds, `time_of_day` is the normalized local solar time in `[0, 1)`,
//! directions are unit vectors in Y-up world space pointing *toward* the light, and colors are linear RGB.
//!
//! The system owns only its clock; frames are independent snapshots, so callers may keep or mutate them freely.

use std::f64::consts::PI;

use crate::core::{
    Rgb,
    smoothstep,
};
use glam::Vec3;
use serde::{
    Deserialize,
    Serialize,
};

use crate::core::format::format_bound;

// ---------------------------------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------------------------------

/// Smallest positive `f64` (JavaScript `Number.MIN_VALUE`); used as the exclusive-zero lower bound of strictly
/// positive options.
const MIN_POSITIVE_VALUE: f64 = 5e-324;

/// Sun angular radius in radians.
const SUN_ANGULAR_RADIUS: f64 = 0.00465;

/// Moon angular radius in radians.
const MOON_ANGULAR_RADIUS: f64 = 0.00436;

/// Peak sun intensity at full daylight.
const SUN_PEAK_INTENSITY: f64 = 5.0;

/// Peak moon intensity at full night.
const MOON_PEAK_INTENSITY: f64 = 0.12;

/// Daylight fraction at which the sun replaces the moon as the primary (shadow-casting) light.
const PRIMARY_SUN_THRESHOLD: f64 = 0.05;

/// Sun color near the horizon (linear RGB, deliberately above one for HDR warmth).
const SUN_HORIZON_COLOR: [f64; 3] = [1.35, 0.35, 0.08];

/// Sun color at full daylight.
const SUN_DAYLIGHT_COLOR: [f64; 3] = [1.0, 0.96, 0.86];

/// Moonlight color.
const MOON_COLOR: [f64; 3] = [0.24, 0.32, 0.55];

/// Sky color at full night.
const NIGHT_SKY_COLOR: [f64; 3] = [0.008, 0.012, 0.035];

/// Sky color at full daylight.
const DAY_SKY_COLOR: [f64; 3] = [0.2, 0.42, 0.8];

/// Additive twilight glow color, scaled by the twilight factor.
const TWILIGHT_GLOW_COLOR: [f64; 3] = [0.8, 0.16, 0.035];

/// Twilight glow contribution at the peak of twilight.
const TWILIGHT_GLOW_SCALE: f64 = 0.35;

/// Skylight intensity at night.
const SKYLIGHT_NIGHT_INTENSITY: f64 = 0.03;

/// Additional skylight intensity at full daylight.
const SKYLIGHT_DAY_INTENSITY: f64 = 0.42;

/// Day of year of the March equinox used by the declination approximation.
const EQUINOX_DAY_OF_YEAR: f64 = 81.0;

/// Days per year used by the declination approximation.
const DAYS_PER_YEAR: f64 = 365.0;

// ---------------------------------------------------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------------------------------------------------

/// Wind policy. Speeds are meters per second; the direction is radians around +Y from +Z toward +X.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct WindOptions {
    /// Direction the wind blows toward, in radians around +Y measured from +Z toward +X.
    pub direction_radians: f64,
    /// Mean wind speed in meters per second; nonnegative.
    pub speed: f64,
    /// Gust amplitude as a fraction of the mean speed, in `[0, 1]`.
    pub gust_strength: f64,
    /// Gust cycle length in seconds; strictly positive.
    pub gust_period_seconds: f64,
}

impl Default for WindOptions {
    fn default() -> Self {
        Self { direction_radians: 0.0, speed: 4.0, gust_strength: 0.25, gust_period_seconds: 8.0 }
    }
}

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

/// Complete environment policy. Serialized names match the TypeScript engine; omitted fields (including fields of
/// nested records) take the TypeScript `DEFAULT_OPTIONS` values, and unknown fields are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct EnvironmentOptions {
    /// Initial normalized local solar time in `[0, 1)`; 0.5 is solar noon.
    pub time_of_day: f64,
    /// Real seconds per simulated day; zero pauses the solar clock.
    pub day_length_seconds: f64,
    /// Observer latitude in degrees, in `[-90, 90]`.
    pub latitude_degrees: f64,
    /// Planet axial tilt in degrees, in `[-45, 45]`.
    pub axial_tilt_degrees: f64,
    /// Day of year in `[1, 366]`, used for solar declination.
    pub day_of_year: f64,
    /// Rotation of geographic north around +Y in radians.
    pub north_radians: f64,
    /// Multiplier applied to elapsed seconds before advancing the solar clock; nonnegative.
    pub time_scale: f64,
    /// Wind policy.
    pub wind: WindOptions,
    /// Atmosphere policy.
    pub atmosphere: AtmosphereOptions,
    /// Cloud policy.
    pub clouds: CloudOptions,
    /// Light shaft policy.
    pub light_shafts: LightShaftOptions,
    /// Ambient occlusion policy.
    pub ambient_occlusion: AmbientOcclusionOptions,
}

impl Default for EnvironmentOptions {
    fn default() -> Self {
        Self {
            time_of_day: 0.35,
            day_length_seconds: 1200.0,
            latitude_degrees: 42.0,
            axial_tilt_degrees: 23.44,
            day_of_year: 172.0,
            north_radians: 0.0,
            time_scale: 1.0,
            wind: WindOptions::default(),
            atmosphere: AtmosphereOptions::default(),
            clouds: CloudOptions::default(),
            light_shafts: LightShaftOptions::default(),
            ambient_occlusion: AmbientOcclusionOptions::default(),
        }
    }
}

impl EnvironmentOptions {
    /// Read options from an authored JSON value. Omitted fields keep their defaults; unknown fields, wrong types, and
    /// out-of-range values are rejected. The input is never retained.
    pub fn from_json(value: &serde_json::Value) -> Result<Self, EnvironmentError> {
        let options: Self = serde_json::from_value(value.clone())
            .map_err(|error| EnvironmentError::InvalidOptions(error.to_string()))?;
        options.validate()?;
        Ok(options)
    }

    /// Check every option against the TypeScript engine's ranges, in the same order.
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        check_range(self.time_of_day, 0.0, 1.0, "timeOfDay")?;
        if self.time_of_day == 1.0 {
            return Err(EnvironmentError::TimeOfDayNotBelowOne);
        }
        check_range(self.day_length_seconds, 0.0, f64::INFINITY, "dayLengthSeconds")?;
        check_range(self.latitude_degrees, -90.0, 90.0, "latitudeDegrees")?;
        check_range(self.axial_tilt_degrees, -45.0, 45.0, "axialTiltDegrees")?;
        check_range(self.day_of_year, 1.0, 366.0, "dayOfYear")?;
        check_range(self.north_radians, f64::NEG_INFINITY, f64::INFINITY, "northRadians")?;
        check_range(self.time_scale, 0.0, f64::INFINITY, "timeScale")?;

        check_range(self.wind.speed, 0.0, f64::INFINITY, "wind.speed")?;
        check_range(self.wind.direction_radians, f64::NEG_INFINITY, f64::INFINITY, "wind.directionRadians")?;
        check_range(self.wind.gust_strength, 0.0, 1.0, "wind.gustStrength")?;
        check_range(self.wind.gust_period_seconds, MIN_POSITIVE_VALUE, f64::INFINITY, "wind.gustPeriodSeconds")?;

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

/// Environment validation failure. Messages match the TypeScript engine.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EnvironmentError {
    /// A value was non-finite or outside its inclusive range.
    #[error("{name} must be finite and between {} and {}.", format_bound(*.minimum), format_bound(*.maximum))]
    OutOfRange {
        /// Serialized option or argument name.
        name: &'static str,
        /// Inclusive lower bound.
        minimum: f64,
        /// Inclusive upper bound.
        maximum: f64,
    },
    /// `timeOfDay` was exactly one; the day interval is half-open.
    #[error("timeOfDay must be less than 1.")]
    TimeOfDayNotBelowOne,
    /// An authored options record had unknown fields or wrong value types.
    #[error("Invalid environment options: {0}")]
    InvalidOptions(String),
}

// ---------------------------------------------------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------------------------------------------------

/// Which body an atmosphere light represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CelestialBody {
    /// The sun.
    Sun,
    /// The moon.
    Moon,
}

/// One celestial directional light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtmosphereLight {
    /// Which body this light represents.
    pub kind: CelestialBody,
    /// Unit vector from the world toward the body, in Y-up world space.
    pub direction: Vec3,
    /// Linear RGB color (may exceed one near the horizon).
    pub color: Rgb,
    /// Linear intensity multiplier.
    pub intensity: f32,
    /// Apparent disc radius in radians.
    pub angular_radius: f32,
    /// Whether the light casts shadows when it is the primary light; always true for environment lights.
    pub casts_shadow: bool,
}

/// Ambient sky contribution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skylight {
    /// Linear RGB sky color.
    pub color: Rgb,
    /// Linear intensity multiplier.
    pub intensity: f32,
}

/// Wind state for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindFrame {
    /// Horizontal unit direction the wind blows toward, in world space.
    pub direction: Vec3,
    /// Wind velocity in meters per second, `direction * speed`.
    pub velocity: Vec3,
    /// Current gusting speed in meters per second; nonnegative.
    pub speed: f32,
}

/// Snapshot of environment state. Owned by the caller; the system never retains or mutates it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentFrame {
    /// Accumulated real seconds since construction (drives wind gusts).
    pub elapsed_seconds: f64,
    /// Normalized local solar time in `[0, 1)`.
    pub time_of_day: f64,
    /// The sun light.
    pub sun: AtmosphereLight,
    /// The moon light (opposite the sun).
    pub moon: AtmosphereLight,
    /// The light that should cast shadows: the sun once daylight reaches 5%, otherwise the moon.
    pub primary_light: AtmosphereLight,
    /// Ambient sky contribution.
    pub skylight: Skylight,
    /// Wind field.
    pub wind: WindFrame,
    /// Copy of the atmosphere policy.
    pub atmosphere: AtmosphereOptions,
    /// Copy of the cloud policy.
    pub clouds: CloudOptions,
    /// Copy of the light shaft policy.
    pub light_shafts: LightShaftOptions,
    /// Copy of the ambient occlusion policy.
    pub ambient_occlusion: AmbientOcclusionOptions,
}

// ---------------------------------------------------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------------------------------------------------

/// Advances deterministic environment state independently from rendering resources.
///
/// The instance owns its clock (`elapsed_seconds`, `time_of_day`) and a validated copy of its options. Independent
/// instances share nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentSystem {
    // Validated policy, copied at construction.
    options: EnvironmentOptions,

    // Clock state.
    elapsed_seconds: f64,
    time_of_day: f64,
}

impl EnvironmentSystem {
    /// Validate and copy environment policy into an independently owned simulation.
    ///
    /// Fails with the first out-of-range option, checked in the TypeScript engine's order.
    pub fn new(options: EnvironmentOptions) -> Result<Self, EnvironmentError> {
        options.validate()?;
        Ok(Self { options, elapsed_seconds: 0.0, time_of_day: wrap_day(options.time_of_day) })
    }

    /// Advance the clock by real elapsed seconds and return the new frame.
    ///
    /// `delta_seconds` must be finite and nonnegative; on failure no state changes. The solar clock advances by
    /// `delta_seconds * time_scale / day_length_seconds` days unless the day length is zero (paused day); wind time
    /// always advances.
    pub fn step(&mut self, delta_seconds: f64) -> Result<EnvironmentFrame, EnvironmentError> {
        check_range(delta_seconds, 0.0, f64::INFINITY, "deltaSeconds")?;
        self.elapsed_seconds += delta_seconds;
        if self.options.day_length_seconds > 0.0 {
            let advance = (delta_seconds * self.options.time_scale) / self.options.day_length_seconds;
            self.time_of_day = wrap_day(self.time_of_day + advance);
        }
        Ok(self.sample())
    }

    /// Set normalized local solar time without changing accumulated wind time.
    ///
    /// `time_of_day` must be finite and in the half-open interval `[0, 1)`; on failure the clock is unchanged.
    pub fn set_time_of_day(&mut self, time_of_day: f64) -> Result<(), EnvironmentError> {
        check_range(time_of_day, 0.0, 1.0, "timeOfDay")?;
        if time_of_day == 1.0 {
            return Err(EnvironmentError::TimeOfDayNotBelowOne);
        }
        self.time_of_day = time_of_day;
        Ok(())
    }

    /// Current normalized local solar time in `[0, 1)`.
    pub fn time_of_day(&self) -> f64 {
        self.time_of_day
    }

    /// Accumulated real seconds that drive wind gusts.
    pub fn elapsed_seconds(&self) -> f64 {
        self.elapsed_seconds
    }

    /// Restore accumulated wind time, for example when loading a saved session.
    ///
    /// Values that are not finite and nonnegative are rejected with a warning and leave the clock unchanged.
    pub fn set_elapsed_seconds(&mut self, elapsed_seconds: f64) {
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            log::warn!("Ignoring invalid environment elapsed seconds {elapsed_seconds}.");
            return;
        }
        self.elapsed_seconds = elapsed_seconds;
    }

    /// The validated options this system was created with.
    pub fn options(&self) -> &EnvironmentOptions {
        &self.options
    }

    /// Produce an environment frame for the current clock without advancing it.
    pub fn sample(&self) -> EnvironmentFrame {
        let sun_direction = calculate_sun_direction(self.time_of_day, &self.options);
        let sun_height = sun_direction[1];
        let daylight = smoothstep(-0.08, 0.12, sun_height);
        let twilight = smoothstep(-0.2, 0.03, sun_height) * (1.0 - smoothstep(0.03, 0.35, sun_height));

        let sun = create_light(
            CelestialBody::Sun,
            sun_direction,
            mix_color(SUN_HORIZON_COLOR, SUN_DAYLIGHT_COLOR, daylight),
            daylight * SUN_PEAK_INTENSITY,
            SUN_ANGULAR_RADIUS,
        );
        let moon_direction = sun_direction.map(|value| -value);
        let moon = create_light(
            CelestialBody::Moon,
            moon_direction,
            MOON_COLOR,
            (1.0 - daylight) * MOON_PEAK_INTENSITY,
            MOON_ANGULAR_RADIUS,
        );
        let primary_light = if daylight >= PRIMARY_SUN_THRESHOLD { sun } else { moon };

        let sky_color = add_color(
            mix_color(NIGHT_SKY_COLOR, DAY_SKY_COLOR, daylight),
            scale_color(TWILIGHT_GLOW_COLOR, twilight * TWILIGHT_GLOW_SCALE),
        );

        EnvironmentFrame {
            elapsed_seconds: self.elapsed_seconds,
            time_of_day: self.time_of_day,
            sun,
            moon,
            primary_light,
            skylight: Skylight {
                color: to_rgb(sky_color),
                intensity: (SKYLIGHT_NIGHT_INTENSITY + (daylight * SKYLIGHT_DAY_INTENSITY)) as f32,
            },
            wind: calculate_wind(self.elapsed_seconds, &self.options.wind),
            atmosphere: self.options.atmosphere,
            clouds: self.options.clouds,
            light_shafts: self.options.light_shafts,
            ambient_occlusion: self.options.ambient_occlusion,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Unit direction toward the sun for a normalized solar time, using a declination approximation.
fn calculate_sun_direction(time_of_day: f64, options: &EnvironmentOptions) -> [f64; 3] {
    // Operation order matches the TypeScript source so results agree to the last few bits.
    let latitude = options.latitude_degrees * PI / 180.0;
    let declination = options.axial_tilt_degrees * PI / 180.0
        * ((2.0 * PI * (options.day_of_year - EQUINOX_DAY_OF_YEAR)) / DAYS_PER_YEAR).sin();
    let hour_angle = (time_of_day - 0.5) * 2.0 * PI;
    let elevation =
        ((latitude.sin() * declination.sin()) + (latitude.cos() * declination.cos() * hour_angle.cos())).asin();
    let azimuth = (-hour_angle.sin()).atan2((declination.tan() * latitude.cos()) - (latitude.sin() * hour_angle.cos()))
        + options.north_radians;
    let horizontal = elevation.cos();
    [azimuth.sin() * horizontal, elevation.sin(), azimuth.cos() * horizontal]
}

/// Gusting wind for an accumulated time; two incommensurate sines keep gusts from looking periodic.
fn calculate_wind(elapsed_seconds: f64, options: &WindOptions) -> WindFrame {
    let phase = elapsed_seconds / options.gust_period_seconds;
    let gust = ((phase * 2.0 * PI).sin() + ((phase * PI * 0.73) + 1.7).sin() * 0.5) / 1.5;
    let speed = 0.0_f64.max(options.speed * (1.0 + (gust * options.gust_strength)));
    let direction = [options.direction_radians.sin(), 0.0, options.direction_radians.cos()];
    let velocity = direction.map(|value| value * speed);
    WindFrame { direction: to_vec3(direction), velocity: to_vec3(velocity), speed: speed as f32 }
}

/// Build one light record, narrowing the `f64` math to stored `f32` values.
fn create_light(
    kind: CelestialBody,
    direction: [f64; 3],
    color: [f64; 3],
    intensity: f64,
    angular_radius: f64,
) -> AtmosphereLight {
    AtmosphereLight {
        kind,
        direction: to_vec3(direction),
        color: to_rgb(color),
        intensity: intensity as f32,
        angular_radius: angular_radius as f32,
        casts_shadow: true,
    }
}

/// Linear interpolation between two colors.
fn mix_color(a: [f64; 3], b: [f64; 3], amount: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * amount, a[1] + (b[1] - a[1]) * amount, a[2] + (b[2] - a[2]) * amount]
}

/// Component-wise color sum.
fn add_color(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Uniform color scale.
fn scale_color(color: [f64; 3], scale: f64) -> [f64; 3] {
    [color[0] * scale, color[1] * scale, color[2] * scale]
}

/// Wrap a day fraction into `[0, 1)` exactly like the TypeScript `((value % 1) + 1) % 1`.
fn wrap_day(value: f64) -> f64 {
    ((value % 1.0) + 1.0) % 1.0
}

/// Narrow an `f64` triple to a vector.
fn to_vec3(value: [f64; 3]) -> Vec3 {
    Vec3::new(value[0] as f32, value[1] as f32, value[2] as f32)
}

/// Narrow an `f64` triple to a stored color.
fn to_rgb(value: [f64; 3]) -> Rgb {
    [value[0] as f32, value[1] as f32, value[2] as f32]
}

/// Reject non-finite or out-of-range values with the TypeScript message.
fn check_range(value: f64, minimum: f64, maximum: f64, name: &'static str) -> Result<(), EnvironmentError> {
    if !value.is_finite() || value < minimum || value > maximum {
        return Err(EnvironmentError::OutOfRange { name, minimum, maximum });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_authored_records_keep_defaults_and_reject_unknown_fields() {
        let options =
            EnvironmentOptions::from_json(&serde_json::json!({"timeOfDay": 0.5, "wind": {"speed": 9}})).unwrap();
        assert_eq!(options.time_of_day, 0.5);
        assert_eq!(options.wind.speed, 9.0);
        assert_eq!(options.wind.gust_period_seconds, 8.0);
        assert_eq!(options.clouds, CloudOptions::default());

        let unknown = EnvironmentOptions::from_json(&serde_json::json!({"weather": "rain"})).unwrap_err();
        assert!(unknown.to_string().contains("weather"), "{unknown}");
        let nested = EnvironmentOptions::from_json(&serde_json::json!({"clouds": {"coverage": 2}})).unwrap_err();
        assert!(nested.to_string().contains("clouds.coverage"), "{nested}");
    }

    #[test]
    fn invalid_policy_and_steps_leave_state_unchanged() {
        let options = EnvironmentOptions { time_of_day: 1.0, ..Default::default() };
        assert_eq!(EnvironmentSystem::new(options).unwrap_err(), EnvironmentError::TimeOfDayNotBelowOne);
        let gusts = EnvironmentOptions {
            wind: WindOptions { gust_period_seconds: 0.0, ..Default::default() },
            ..Default::default()
        };
        assert!(EnvironmentSystem::new(gusts).unwrap_err().to_string().contains("gustPeriodSeconds"));

        let mut environment = EnvironmentSystem::new(EnvironmentOptions::default()).unwrap();
        let before = environment.clone();
        assert!(environment.step(-1.0).unwrap_err().to_string().contains("deltaSeconds"));
        assert!(environment.step(f64::NAN).is_err());
        assert_eq!(environment.set_time_of_day(1.0), Err(EnvironmentError::TimeOfDayNotBelowOne));
        environment.set_elapsed_seconds(f64::INFINITY);
        assert_eq!(environment, before);
    }

    #[test]
    fn paused_day_still_advances_wind_time() {
        let options = EnvironmentOptions { time_of_day: 0.5, day_length_seconds: 0.0, ..Default::default() };
        let mut environment = EnvironmentSystem::new(options).unwrap();
        let frame = environment.step(60.0).unwrap();
        assert_eq!(frame.time_of_day, 0.5);
        assert_eq!(frame.elapsed_seconds, 60.0);
    }
}
