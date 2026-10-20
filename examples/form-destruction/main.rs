// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM DESTRUCTION: inspect wall integrity, apply cumulative damage, and keep the resulting rubble. A seeded chunk
//! structure takes radial damage, loses unsupported overhangs, and releases rigid debris that settles and stays.
//!
//!     cargo run --example form-destruction
//!     node scripts/rust/build-web.mjs --example form-destruction
//!
//! Click a surface to strike it, drag to orbit, scroll to zoom, and press R to reset.

#[path = "src/mod.rs"]
mod example;

use example::destruction_demo::{
    FormDestructionDemo,
    input_map,
};
use joy_engine::prelude::*;

/// Shell settings: default renderer with click-or-drag input, reset, and cancel.
fn options() -> ShellOptions {
    ShellOptions {
        title: "FORM DESTRUCTION".into(),
        input: input_map(),
        ..Default::default()
    }
}

fn start() {
    let result = FormDestructionDemo::new()
        .map_err(|error| ShellError::App(error.to_string()))
        .and_then(|demo| run(options(), demo));
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
