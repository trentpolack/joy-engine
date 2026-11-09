// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rust integration tests for Joy Engine, one module per subsystem, built as a single test binary so the engine links
//! once. Run with `cargo test`. GPU modules skip with a message when no adapter (hardware or software) is available;
//! set `JOY_RENDER_CAPTURE_DIR` to keep their PNG captures. Tooling tests live beside this folder in `tests/tooling/`.

// Modules that drive the authoring kit, development panels, or the console need the features that provide them.
#[cfg(feature = "authoring")]
mod development_kit;
mod example_form_building;
#[cfg(feature = "devtools")]
mod example_form_destruction;
#[cfg(feature = "devtools")]
mod example_scene_effects;
mod example_sdf_collisions;
#[cfg(feature = "devtools")]
mod example_terrain;
mod fixed_tick;
#[cfg(feature = "devtools")]
mod rendering_scene_effects;
