// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Light records shared by simulation and rendering: directional lights, point lights, and the skylight.
//!
//! These are plain records in Y-up world space with linear colors and linear intensity multipliers. The environment
//! simulation produces directional lights and the skylight (its `AtmosphereLight` is a [`DirectionalLight`] plus the
//! celestial body it represents), level `light` components and particles produce [`PointLight`]s, and the renderer
//! lights and shadows surfaces with all three.

use crate::color::LinearColor;
use crate::math::Vec3;

/// Apparent angular radius of the sun in radians; the default source size of a [`DirectionalLight`].
pub const SUN_ANGULAR_RADIUS: f32 = 0.00465;

/// Largest accepted directional source radius in radians, a quarter turn (a light spanning the whole hemisphere).
const MAX_ANGULAR_RADIUS: f32 = std::f32::consts::FRAC_PI_2;

/// Rec. 709 luminance weights for linear RGB.
const LUMINANCE_WEIGHTS: [f32; 3] = [0.2126, 0.7152, 0.0722];

// ---------------------------------------------------------------------------------------------------------------------
// Directional lights
// ---------------------------------------------------------------------------------------------------------------------

/// A light infinitely far away whose parallel rays all arrive from one direction: the base record for the sun, the
/// moon, and studio key lights.
///
/// The renderer lights every surface with it and, when [`casts_shadow`](Self::casts_shadow) is set, renders cascaded
/// soft shadows for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    /// Unit vector from the world toward the light, in Y-up world space.
    pub direction: Vec3,
    /// Linear color before intensity; alpha is unused. Channels may exceed one for HDR tints.
    pub color: LinearColor,
    /// Linear intensity multiplier; zero turns the light off.
    pub intensity: f32,
    /// Apparent radius of the light's source, in radians. Every directional light has one, not only celestial ones:
    /// it sets how quickly shadow penumbrae widen with distance from their caster (a larger source casts softer
    /// shadows). Atmosphere lights also draw their disc in the sky at this size.
    pub angular_radius: f32,
    /// Whether the light casts directional (cascaded) shadows.
    pub casts_shadow: bool,
}

impl Default for DirectionalLight {
    /// White light from directly overhead with the sun's source size, casting shadows.
    fn default() -> Self {
        Self {
            direction: Vec3::Y,
            color: LinearColor::WHITE,
            intensity: 1.0,
            angular_radius: SUN_ANGULAR_RADIUS,
            casts_shadow: true,
        }
    }
}

impl DirectionalLight {
    /// A shadow-casting light toward `direction` (normalized here; a zero vector points straight up) with the sun's
    /// source size.
    pub fn new(direction: Vec3, color: LinearColor, intensity: f32) -> Self {
        Self { direction: direction.normalize_or(Vec3::Y), color, intensity, ..Self::default() }
    }

    /// `color * intensity`: the irradiance a surface facing the light receives.
    pub fn radiance(&self) -> LinearColor {
        self.color.scaled(self.intensity)
    }

    /// Whether the light contributes anything: a positive intensity and a color that is not black.
    pub fn is_lit(&self) -> bool {
        self.intensity > 0.0 && self.color.rgb_array().iter().any(|channel| *channel > 0.0)
    }

    /// Whether every value is usable: finite, a nonzero direction, nonnegative color and intensity, and a source
    /// radius in `[0, π/2]`.
    pub fn is_valid(&self) -> bool {
        let color = self.color.rgb_array();
        self.direction.is_finite()
            && self.direction.length_squared() > 0.0
            && color.iter().all(|channel| channel.is_finite() && *channel >= 0.0)
            && self.intensity.is_finite()
            && self.intensity >= 0.0
            && (0.0..=MAX_ANGULAR_RADIUS).contains(&self.angular_radius)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Point lights
// ---------------------------------------------------------------------------------------------------------------------

/// A light radiating from one point whose contribution fades to zero at its range.
///
/// Positions and range are world units. Level `light` components and particle emitters both produce point lights;
/// particles default to not casting shadows, but the choice is theirs to make like any other light's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// World position.
    pub position: Vec3,
    /// Linear irradiance at the light (color multiplied by intensity); alpha is unused.
    pub color: LinearColor,
    /// Distance at which the light's contribution reaches zero; must be positive.
    pub range: f32,
    /// Whether the light casts soft shadows from six depth faces.
    pub casts_shadow: bool,
}

impl PointLight {
    /// Rec. 709 luminance of the light's color, the key lights are ranked by when there are more than a renderer draws.
    pub fn luminance(&self) -> f32 {
        let [red, green, blue] = self.color.rgb_array();
        (red * LUMINANCE_WEIGHTS[0]) + (green * LUMINANCE_WEIGHTS[1]) + (blue * LUMINANCE_WEIGHTS[2])
    }

    /// Whether every value is usable: a finite position, a finite positive range, and finite nonnegative color.
    pub fn is_valid(&self) -> bool {
        let color = self.color.rgb_array();
        self.position.is_finite()
            && self.range.is_finite()
            && self.range > 0.0
            && color.iter().all(|channel| channel.is_finite() && *channel >= 0.0)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Skylight
// ---------------------------------------------------------------------------------------------------------------------

/// Ambient light from the sky dome above the horizon and the ground below it.
///
/// A surface facing straight up sees only the sky, one facing straight down sees only the ground, and everything in
/// between blends the two by how far its normal tilts (the exact irradiance of two uniform hemispheres). The ground
/// color is the light the ground reflects back up; a skylight whose ground color equals its sky color lights every
/// direction equally.
///
/// A skylight does not cast shadows the way the sun does: it arrives from every direction at once, so its occlusion is
/// ambient occlusion (material occlusion maps and the screen-space ambient occlusion scene effects), not a shadow map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skylight {
    /// Linear radiance of the sky overhead; alpha is unused.
    pub color: LinearColor,
    /// Linear radiance reflected up from below the horizon; alpha is unused.
    pub ground_color: LinearColor,
    /// Linear intensity multiplier for both hemispheres.
    pub intensity: f32,
}

impl Default for Skylight {
    /// No ambient light.
    fn default() -> Self {
        Self { color: LinearColor::BLACK, ground_color: LinearColor::BLACK, intensity: 0.0 }
    }
}

impl Skylight {
    /// A skylight with the same radiance from every direction.
    pub fn uniform(color: LinearColor, intensity: f32) -> Self {
        Self { color, ground_color: color, intensity }
    }

    /// Irradiance (before albedo) on a surface whose world-space unit normal is `normal`; the shaders evaluate the same
    /// blend per pixel.
    pub fn irradiance(&self, normal: Vec3) -> LinearColor {
        let sky_weight = (0.5 + (0.5 * normal.y)).clamp(0.0, 1.0);
        let blend = |ground: f32, sky: f32| (ground + ((sky - ground) * sky_weight)) * self.intensity;
        let [sky_red, sky_green, sky_blue] = self.color.rgb_array();
        let [ground_red, ground_green, ground_blue] = self.ground_color.rgb_array();
        LinearColor::rgb(blend(ground_red, sky_red), blend(ground_green, sky_green), blend(ground_blue, sky_blue))
    }
}

#[cfg(test)]
#[path = "light.test.rs"]
mod tests;
