// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM BUILDING: generate and reshape a building shell from an editable FORM recipe. One source makes cottage,
//! warehouse, and apartment exteriors; the engine renderer draws their PBR materials.
//!
//!     cargo run --example form-building
//!     cargo run --example form-building -- --mode=baked
//!     node scripts/rust/build-web.mjs --example form-building
//!
//! Drag to orbit and scroll to zoom. In browsers, `?mode=baked` loads the baked cottage without evaluating FORM.

#[path = "src/mod.rs"]
mod example;

use example::building_example::{
    BuildingExample,
    ORBIT_ACTION,
};
use joy_engine::prelude::*;

/// Shell settings: default renderer and drag-to-orbit input.
fn options() -> ShellOptions {
    let input = InputMap::new().action(ORBIT_ACTION, [InputBinding::MouseButton(0)]);
    ShellOptions {
        title: "FORM BUILDING".into(),
        input,
        ..Default::default()
    }
}

fn start() {
    let result = BuildingExample::new(BuildingExample::launch_mode())
        .map_err(|error| ShellError::App(error.to_string()))
        .and_then(|example| run(options(), example));
    if let Err(error) = result {
        joy_engine::log::error!("{error}");
    }
}

joy_engine::web_main!(start);

fn main() {
    // Browsers start through `web_main!` instead.
    #[cfg(not(target_arch = "wasm32"))]
    start();
}
