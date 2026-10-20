// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Content layer: glTF 2.0 and GLB digital content creation (DCC) imports that turn exported assets into renderer-free
//! mesh records. Each DCC format is its own crate (`joy-dcc-<format>`).

pub mod glb_mesh_loader;

pub use glb_mesh_loader::{
    GlbError,
    parse_glb_mesh,
};
