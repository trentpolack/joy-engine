// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Point-light data for the scene passes: light clusters built on the CPU each frame, and the textures that carry them
//! to every pass that lights surfaces or media (forward and deferred shading, terrain, and volumetric fog).
//!
//! Textures rather than storage buffers keep one path on every backend, WebGL 2 included.

mod clusters;
mod textures;

pub(crate) use clusters::{
    CLUSTER_COLUMNS,
    CLUSTER_ROWS,
    CLUSTER_SLICES,
    LightClusters,
};
pub(crate) use textures::LightTextures;
