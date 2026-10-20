// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! SCENE EFFECTS: a reproducible room for toggling the renderer's scene effects (ambient occlusion, color bleed,
//! outlines, temporal antialiasing, motion blur, reflections, fog, clustered volumetric lighting, adaptive exposure,
//! and HDR bloom) over 27 shadowed point lights.
//!
//!     cargo run --example scene-effects
//!     node scripts/rust/build-web.mjs --example scene-effects
//!
//! Drag to orbit and scroll to zoom. Scene effects need a WebGPU-class backend (Vulkan, Metal, DirectX 12, or browser
//! WebGPU); with `?backend=webgl` the controls report them unavailable and HDR bloom remains.

#[path = "src/mod.rs"]
mod example;

use example::config;
use example::scene_effects_example::{
    ORBIT_ACTION,
    SceneEffectsExample,
};
use joy_engine::prelude::*;

/// Shell settings: the lab's display baseline, effects created off, and drag-to-orbit input.
fn options() -> ShellOptions {
    let input = InputMap::new().action(ORBIT_ACTION, [InputBinding::MouseButton(0)]);
    ShellOptions {
        title: "SCENE EFFECTS".into(),
        renderer: config::renderer_options(),
        input,
        ..Default::default()
    }
}

fn start() {
    if let Err(error) = run(options(), SceneEffectsExample::new()) {
        joy_engine::log::error!("{error}");
    }
}

joy_engine::web_main!(start);

fn main() {
    // Browsers start through `web_main!` instead.
    #[cfg(not(target_arch = "wasm32"))]
    start();
}
