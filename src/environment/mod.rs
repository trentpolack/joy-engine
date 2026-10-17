// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Environment simulation (simulation layer): solar time, sun and moon lights, skylight, wind, and the atmosphere,
//! cloud, light-shaft, and ambient-occlusion policies the renderer reads. Headless; time is in seconds.

mod environment_system;

pub use environment_system::{
    AmbientOcclusionOptions,
    AtmosphereLight,
    AtmosphereOptions,
    CelestialBody,
    CloudOptions,
    EnvironmentError,
    EnvironmentFrame,
    EnvironmentOptions,
    EnvironmentSystem,
    LightShaftOptions,
    Skylight,
    WindFrame,
    WindOptions,
};
