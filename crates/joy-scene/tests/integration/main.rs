// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Integration tests for `joy-scene`: presentation component schemas, post-process volumes, FORM mesh assets and
//! libraries, and asset scenes that present levels with per-entity particle effects. Run with
//! `cargo test -p joy-scene`. The particle fixtures, including the golden particle recording, are shared with
//! `joy-renderer`'s particle tests.

mod particles_asset_scene;
mod post_process_volumes;
mod rendering_form_assets;
mod scene_components;
