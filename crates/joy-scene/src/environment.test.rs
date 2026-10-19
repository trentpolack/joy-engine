// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/environment.rs`.

use joy_environment::{
    EnvironmentOptions,
    EnvironmentSystem,
};
use joy_renderer::CloudOptions;

use super::*;

#[test]
fn packed_environment_matches_the_shader_uniform_layout() {
    let options = EnvironmentOptions { time_of_day: 0.5, ..Default::default() };
    let frame = EnvironmentSystem::new(options).unwrap().step(2.0).unwrap();
    let sky = SkyOptions { clouds: CloudOptions { shadow_strength: 0.3, ..Default::default() }, ..Default::default() };
    let packed = environment_lighting(&frame, &sky).pack();

    assert_eq!(&packed[0][..3], &frame.sun.light.direction.to_array());
    assert_eq!(packed[0][3], frame.sun.light.intensity);
    assert_eq!(&packed[3][..3], &frame.moon.light.color.rgb_array());
    assert_eq!(packed[4][3], frame.skylight.intensity);
    assert_eq!(packed[5][3], frame.elapsed_seconds as f32);
    assert_eq!(packed[6][0], sky.atmosphere.rayleigh as f32);
    assert_eq!(packed[7][3], sky.clouds.shadow_strength as f32);
    assert_eq!(packed[8][3], sky.light_shafts.decay as f32);
    assert_eq!(
        packed[9],
        [
            sky.light_shafts.exposure as f32,
            sky.ambient_occlusion.intensity as f32,
            sky.ambient_occlusion.radius as f32,
            frame.time_of_day as f32,
        ]
    );
    assert!(packed[10..].iter().flatten().all(|value| *value == 0.0));
}
