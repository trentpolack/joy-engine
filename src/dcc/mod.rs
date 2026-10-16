// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Content layer: digital content creation (DCC) imports that turn exported assets into renderer-free mesh records.

pub mod glb_mesh_loader;

pub use glb_mesh_loader::{GlbError, parse_glb_mesh};
