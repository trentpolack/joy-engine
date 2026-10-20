// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Image-based lighting: prepares environment maps for the renderer's lighting record. `joy-scene` fills that record
//! from the environment simulation and [`SkyOptions`](crate::SkyOptions).

mod capture;
mod image_lighting;
mod irradiance;

pub use image_lighting::{
    EnvironmentData,
    EnvironmentEncoding,
    EnvironmentMap,
    EnvironmentMapOptions,
    EnvironmentPixels,
    ImageLighting,
};
pub(crate) use image_lighting::{
    ImageLightingResources,
    image_lighting_layout,
};
