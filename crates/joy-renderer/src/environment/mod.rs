// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Image-based lighting: prepares environment maps for the renderer's lighting record. `joy-scene` fills that record
//! from the environment simulation and [`SkyOptions`](crate::SkyOptions).

mod image_lighting;

pub use image_lighting::{
    EnvironmentData,
    EnvironmentEncoding,
    EnvironmentMap,
    EnvironmentMapOptions,
    EnvironmentPixels,
    ImageLighting,
};
pub(crate) use image_lighting::{
    environment_format,
    image_lighting_group,
    image_lighting_layout,
    lighting_textures,
    lighting_views,
    prepare_environment_map,
};
