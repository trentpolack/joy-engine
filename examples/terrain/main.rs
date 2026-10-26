// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Terrain: Dragon Pit streamed as a paged quadtree with synthesized detail, craters, and terrain physics.
//!
//!     cargo run --example terrain
//!     cargo run --example terrain -- --package=<package folder>
//!     node scripts/rust/build-web.mjs --example terrain
//!
//! Browsers prefer WebGPU and fall back to WebGL 2; add `?backend=webgl` or `?backend=webgpu` to choose, and
//! `?package=<url prefix>` to stream a package written by `joy-terrain-package`. See `src/terrain_lab.rs` for controls.
//!
//! Demo dimensions are explicit example units (X/Z extent 2048, height = source height x 1000), not physical meters.

#[path = "src/mod.rs"]
mod example;

use example::terrain_lab::{
    TerrainLab,
    input_map,
    renderer_options,
};
use joy_engine::prelude::*;

/// Shell settings: the lab's keys and a shadowed, atmospheric renderer.
fn options() -> ShellOptions {
    // The default canvas ID matches `scripts/rust/build-web.mjs` pages built without `--canvas`.
    ShellOptions {
        title: "Joy Terrain Lab".into(),
        input: input_map(),
        renderer: renderer_options(),
        ..Default::default()
    }
}

fn start() {
    let result =
        TerrainLab::new().map_err(|error| ShellError::App(error.to_string())).and_then(|app| run(options(), app));
    if let Err(error) = result {
        joy_engine::log::error!("Terrain startup failed: {error}");
    }
}

joy_engine::web_main!(start);

fn main() {
    // Browsers start through `web_main!` instead.
    #[cfg(not(target_arch = "wasm32"))]
    start();
}
