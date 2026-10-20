// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Deterministic world environment: a solar clock, celestial lights, a sky-and-ground skylight, and a gusting wind field.
//!
//! Every quantity is computed in `f64` in a fixed operation order and only then stored as `f32` in the frame, so frames
//! reproduce exactly on every build and target; golden recordings check them within single-precision tolerance. Time
//! values are seconds, `time_of_day` is the normalized local solar time in `[0, 1)`, directions are unit vectors in
//! Y-up world space pointing *toward* the light, and colors are linear RGB.
//!
//! The system owns only its clock; frames are independent snapshots, so callers may keep or mutate them freely.

use std::f64::consts::PI;

use joy_core::math::Vec3;
use joy_core::math::deterministic;
use joy_core::{
    LinearColor,
    smoothstep,
};
use joy_render_types::{
    DirectionalLight,
    Skylight,
};
use serde::{
    Deserialize,
    Serialize,
};

use joy_core::format::format_number;

// ---------------------------------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------------------------------

/// Smallest positive `f64` (the subnormal `5e-324`, below `f64::MIN_POSITIVE`); used as the exclusive-zero lower bound
/// of strictly positive options.
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

/// Fraction of the light reaching the ground that it reflects back up into the skylight's lower hemisphere; grass,
/// soil, and rock average about 0.2.
const GROUND_ALBEDO: f64 = 0.2;

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
        Self {
            direction_radians: 0.0,
            speed: 4.0,
            gust_strength: 0.25,
            gust_period_seconds: 8.0,
        }
    }
}

/// Environment simulation policy. Fields serialize with camelCase document names; omitted fields (including fields of
/// nested records) take their [`Default`] values, and unknown fields are rejected.
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

    /// Check every option against its range in a fixed order, so the first failure reported is a stable diagnostic.
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
        Ok(())
    }
}

/// Environment validation failure. Messages are stable diagnostics.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EnvironmentError {
    /// A value was non-finite or outside its inclusive range.
    #[error("{name} must be finite and between {} and {}.", format_number(*.minimum), format_number(*.maximum))]
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

/// A celestial body's light: a [`DirectionalLight`] plus the body it comes from.
///
/// The directional light carries everything the renderer lights and shadows with (direction toward the body, color,
/// intensity, source size, and shadow casting); the body tells the sky which disc to draw. Environment lights always
/// cast shadows, and their angular radius is the body's apparent disc radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtmosphereLight {
    /// Which body this light represents.
    pub kind: CelestialBody,
    /// The light the body casts. Its color may exceed one near the horizon.
    pub light: DirectionalLight,
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
    /// Ambient light: the sky overhead, and below the horizon the sky and sun light the ground reflects back up.
    pub skylight: Skylight,
    /// Wind field.
    pub wind: WindFrame,
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
    /// Fails with the first out-of-range option, checked in [`EnvironmentOptions::validate`] order.
    pub fn new(options: EnvironmentOptions) -> Result<Self, EnvironmentError> {
        options.validate()?;
        Ok(Self {
            options,
            elapsed_seconds: 0.0,
            time_of_day: wrap_day(options.time_of_day),
        })
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
        let sky_intensity = SKYLIGHT_NIGHT_INTENSITY + (daylight * SKYLIGHT_DAY_INTENSITY);
        let ground_color = ground_bounce_color(sky_color, sky_intensity, &[&sun.light, &moon.light]);

        EnvironmentFrame {
            elapsed_seconds: self.elapsed_seconds,
            time_of_day: self.time_of_day,
            sun,
            moon,
            primary_light,
            skylight: Skylight {
                color: to_linear_color(sky_color),
                ground_color: to_linear_color(ground_color),
                intensity: sky_intensity as f32,
            },
            wind: calculate_wind(self.elapsed_seconds, &self.options.wind),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Unit direction toward the sun for a normalized solar time, using a declination approximation.
fn calculate_sun_direction(time_of_day: f64, options: &EnvironmentOptions) -> [f64; 3] {
    // Operation order is fixed so results reproduce exactly on every build and target.
    let latitude = options.latitude_degrees * PI / 180.0;
    let declination = options.axial_tilt_degrees * PI / 180.0
        * deterministic::sin((2.0 * PI * (options.day_of_year - EQUINOX_DAY_OF_YEAR)) / DAYS_PER_YEAR);
    let hour_angle = (time_of_day - 0.5) * 2.0 * PI;
    let elevation = deterministic::asin(
        (deterministic::sin(latitude) * deterministic::sin(declination))
            + (deterministic::cos(latitude) * deterministic::cos(declination) * deterministic::cos(hour_angle)),
    );
    let azimuth = deterministic::atan2(
        -deterministic::sin(hour_angle),
        (deterministic::tan(declination) * deterministic::cos(latitude))
            - (deterministic::sin(latitude) * deterministic::cos(hour_angle)),
    ) + options.north_radians;
    let horizontal = deterministic::cos(elevation);
    [deterministic::sin(azimuth) * horizontal, deterministic::sin(elevation), deterministic::cos(azimuth) * horizontal]
}

/// Gusting wind for an accumulated time; two incommensurate sines keep gusts from looking periodic.
fn calculate_wind(elapsed_seconds: f64, options: &WindOptions) -> WindFrame {
    let phase = elapsed_seconds / options.gust_period_seconds;
    let gust = (deterministic::sin(phase * 2.0 * PI) + deterministic::sin((phase * PI * 0.73) + 1.7) * 0.5) / 1.5;
    let speed = 0.0_f64.max(options.speed * (1.0 + (gust * options.gust_strength)));
    let direction = [deterministic::sin(options.direction_radians), 0.0, deterministic::cos(options.direction_radians)];
    let velocity = direction.map(|value| value * speed);
    WindFrame {
        direction: to_vec3(direction),
        velocity: to_vec3(velocity),
        speed: speed as f32,
    }
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
        light: DirectionalLight {
            direction: to_vec3(direction),
            color: to_linear_color(color),
            intensity: intensity as f32,
            angular_radius: angular_radius as f32,
            casts_shadow: true,
        },
    }
}

/// The skylight's lower-hemisphere color: the light a flat, diffuse ground reflects back up, before the skylight's
/// intensity.
///
/// The ground receives the sky dome (irradiance `π × sky radiance`) and every celestial light above the horizon
/// (`radiance × elevation`), and reflects `GROUND_ALBEDO` of it evenly; dividing by `π` turns that back into radiance.
/// Under a high sun the bounce is about as bright as the sky itself and warmer, and at night it falls well below it.
fn ground_bounce_color(sky_color: [f64; 3], sky_intensity: f64, lights: &[&DirectionalLight]) -> [f64; 3] {
    let mut bounce = scale_color(sky_color, sky_intensity);
    for light in lights {
        let elevation = f64::from(light.direction.y).max(0.0);
        let strength = (f64::from(light.intensity) * elevation) / PI;
        let [red, green, blue] = light.color.rgb_array().map(f64::from);
        bounce = add_color(bounce, [red * strength, green * strength, blue * strength]);
    }
    scale_color(bounce, GROUND_ALBEDO / sky_intensity.max(MIN_POSITIVE_VALUE))
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

/// Wrap a day fraction into `[0, 1)`; negative fractions wrap forward from the end of the day.
fn wrap_day(value: f64) -> f64 {
    ((value % 1.0) + 1.0) % 1.0
}

/// Narrow an `f64` triple to a vector.
fn to_vec3(value: [f64; 3]) -> Vec3 {
    Vec3::new(value[0] as f32, value[1] as f32, value[2] as f32)
}

/// Narrow an `f64` linear RGB triple to an opaque stored color.
fn to_linear_color(value: [f64; 3]) -> LinearColor {
    LinearColor::rgb(value[0] as f32, value[1] as f32, value[2] as f32)
}

/// Reject non-finite or out-of-range values with the stable range diagnostic.
fn check_range(value: f64, minimum: f64, maximum: f64, name: &'static str) -> Result<(), EnvironmentError> {
    if !value.is_finite() || value < minimum || value > maximum {
        return Err(EnvironmentError::OutOfRange {
            name,
            minimum,
            maximum,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "environment_system.test.rs"]
mod tests;
