// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The SDF collisions example's modules: tuning (`config/`), the FORM props and their colliders, one bin and its pile,
//! the headless benchmark, the sidebar, and the demo itself. `main.rs` and the integration tests in `test/rust/` both
//! include this tree.

/// Named tuning values.
pub mod config {
    pub mod sdf_collisions_config;
}
pub mod benchmark;
pub mod hud;
pub mod pile_arena;
pub mod prop_library;
pub mod sdf_collisions_demo;
