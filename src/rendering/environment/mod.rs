// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Adapts simulation environment frames to the renderer's lighting record, and prepares image-based lighting.

mod image_lighting;

pub use image_lighting::{
    EnvironmentData, EnvironmentEncoding, EnvironmentMap, EnvironmentMapOptions, EnvironmentPixels, ImageLighting,
};
pub(crate) use image_lighting::{
    environment_format, image_lighting_group, image_lighting_layout, lighting_textures, lighting_views,
    prepare_environment_map,
};

use crate::environment::{AtmosphereLight, EnvironmentFrame};

use crate::rendering::frame::{CelestialLight, EnvironmentLighting};

impl From<&AtmosphereLight> for CelestialLight {
    fn from(light: &AtmosphereLight) -> Self {
        Self {
            direction: light.direction,
            color: light.color,
            intensity: light.intensity,
            angular_radius: light.angular_radius,
            casts_shadow: light.casts_shadow,
        }
    }
}

impl From<&EnvironmentFrame> for EnvironmentLighting {
    /// Copy the values the TypeScript `createEnvironmentUniforms` and `createEnvironmentLighting` read, in their order.
    fn from(frame: &EnvironmentFrame) -> Self {
        let atmosphere = &frame.atmosphere;
        let clouds = &frame.clouds;
        let shafts = &frame.light_shafts;
        let occlusion = &frame.ambient_occlusion;
        Self {
            elapsed_seconds: frame.elapsed_seconds as f32,
            time_of_day: frame.time_of_day as f32,
            sun: (&frame.sun).into(),
            moon: (&frame.moon).into(),
            primary: (&frame.primary_light).into(),
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
}

#[cfg(test)]
mod tests {
    use crate::environment::{EnvironmentOptions, EnvironmentSystem};

    use super::*;

    #[test]
    fn packed_environment_matches_the_typescript_uniform_layout() {
        let options = EnvironmentOptions { time_of_day: 0.5, ..Default::default() };
        let frame = EnvironmentSystem::new(options).unwrap().step(2.0).unwrap();
        let packed = EnvironmentLighting::from(&frame).pack();

        assert_eq!(&packed[0][..3], &frame.sun.direction.to_array());
        assert_eq!(packed[0][3], frame.sun.intensity);
        assert_eq!(&packed[3][..3], &frame.moon.color);
        assert_eq!(packed[4][3], frame.skylight.intensity);
        assert_eq!(packed[5][3], frame.elapsed_seconds as f32);
        assert_eq!(packed[6][0], frame.atmosphere.rayleigh as f32);
        assert_eq!(packed[7][3], frame.clouds.shadow_strength as f32);
        assert_eq!(packed[8][3], frame.light_shafts.decay as f32);
        assert_eq!(
            packed[9],
            [
                frame.light_shafts.exposure as f32,
                frame.ambient_occlusion.intensity as f32,
                frame.ambient_occlusion.radius as f32,
                frame.time_of_day as f32,
            ]
        );
        assert!(packed[10..].iter().flatten().all(|value| *value == 0.0));
    }
}
