// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Environment presentation: lights a frame from the environment simulation's sun, moon, skylight, and wind plus the
//! renderer's sky settings. Neither crate knows the other; this is the only place their records meet.

use joy_environment::{
    AtmosphereLight,
    EnvironmentFrame,
};
use joy_renderer::{
    CelestialLight,
    EnvironmentLighting,
    SkyOptions,
};

/// The renderer's lighting record for one frame of `frame`, with `sky` shaping scattering, fog, clouds, light shafts,
/// and ambient occlusion.
///
/// Copies the values the TypeScript `createEnvironmentUniforms` and `createEnvironmentLighting` read, in their order.
/// Validate authored sky settings once with [`SkyOptions::validate`] before lighting frames with them.
pub fn environment_lighting(frame: &EnvironmentFrame, sky: &SkyOptions) -> EnvironmentLighting {
    let atmosphere = &sky.atmosphere;
    let clouds = &sky.clouds;
    let shafts = &sky.light_shafts;
    let occlusion = &sky.ambient_occlusion;
    EnvironmentLighting {
        elapsed_seconds: frame.elapsed_seconds as f32,
        time_of_day: frame.time_of_day as f32,
        sun: celestial_light(&frame.sun),
        moon: celestial_light(&frame.moon),
        primary: celestial_light(&frame.primary_light),
        skylight_color: frame.skylight.color,
        skylight_intensity: frame.skylight.intensity,
        wind_velocity: frame.wind.velocity,
        scattering: [atmosphere.rayleigh, atmosphere.mie, atmosphere.ozone, atmosphere.density].map(|v| v as f32),
        fog_and_clouds: [atmosphere.fog_density, clouds.coverage, clouds.density, clouds.shadow_strength]
            .map(|value| value as f32),
        cloud_shape_and_shafts: [clouds.altitude, clouds.thickness, shafts.intensity, shafts.decay]
            .map(|value| value as f32),
        shaft_exposure_and_occlusion: [shafts.exposure, occlusion.intensity, occlusion.radius]
            .map(|value| value as f32),
    }
}

/// A simulated sun or moon as the renderer's light record.
fn celestial_light(light: &AtmosphereLight) -> CelestialLight {
    CelestialLight {
        direction: light.direction,
        color: light.color,
        intensity: light.intensity,
        angular_radius: light.angular_radius,
        casts_shadow: light.casts_shadow,
    }
}

#[cfg(test)]
#[path = "../tests/unit/environment.rs"]
mod tests;
