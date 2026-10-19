// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Environment simulation (simulation layer): solar time, sun and moon lights (directional lights tagged with their
//! celestial body), the sky-and-ground skylight, and wind. Headless; time is in seconds. Sky presentation settings
//! (atmosphere, clouds, light shafts, and ambient occlusion) belong to the renderer.

mod environment_system;

pub use environment_system::{
    AtmosphereLight,
    CelestialBody,
    EnvironmentError,
    EnvironmentFrame,
    EnvironmentOptions,
    EnvironmentSystem,
    WindFrame,
    WindOptions,
};
pub use joy_core::{
    DirectionalLight,
    Skylight,
};
