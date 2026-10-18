// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Integration tests for `joy-renderer`: headless rendering, materials, shadows, bloom, triangles, terrain, and
//! particle drawing and simulation parity. Run with `cargo test -p joy-renderer`. GPU modules skip with a message when
//! no adapter (hardware or software) is available; set `JOY_RENDER_CAPTURE_DIR` to keep their PNG captures.

mod particles_effect;
mod particles_headless;
mod particles_parity;
mod particles_rendering;
mod rendering_bloom;
mod rendering_headless;
mod rendering_materials;
mod rendering_terrain;
mod rendering_triangles;
mod terrain;
