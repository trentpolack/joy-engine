// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! SDF COLLISIONS: the same FORM props dropped into two bins, one colliding through signed distance fields and one
//! through bounding boxes, to compare how the piles settle and what each costs.
//!
//!     cargo run --example sdf-collisions
//!     cargo run --release --example sdf-collisions -- --bench
//!     node scripts/rust/build-web.mjs --example sdf-collisions
//!
//! Drag to orbit, scroll to zoom, Space to drop more props, P to pause, and R to reset. Natively, `--bench` prints the
//! headless benchmark and exits; in browsers, `?bench=1` logs it to the console and shows it in the sidebar.

#[path = "src/mod.rs"]
mod example;

use example::sdf_collisions_demo::{
    SdfCollisionsDemo,
    input_map,
};
use joy_engine::prelude::*;

/// Shell settings: default renderer with drag-to-orbit and the demo's keys.
fn options() -> ShellOptions {
    ShellOptions {
        title: "SDF COLLISIONS".into(),
        input: input_map(),
        ..Default::default()
    }
}

fn start() {
    let result = SdfCollisionsDemo::new()
        .map_err(|error| ShellError::App(error.to_string()))
        .and_then(|demo| run(options(), demo));
    if let Err(error) = result {
        joy_engine::log::error!("{error}");
    }
}

/// Print the headless benchmark without opening a window.
#[cfg(not(target_arch = "wasm32"))]
fn print_benchmark() {
    use example::prop_library::PropLibrary;
    use example::sdf_collisions_demo::benchmark;

    let report = PropLibrary::load().and_then(|library| benchmark(&library));
    match report {
        Ok(report) => println!("{report}"),
        Err(error) => eprintln!("{error}"),
    }
}

joy_engine::web_main!(start);

fn main() {
    // Browsers start through `web_main!` instead, and run the benchmark inside the demo.
    #[cfg(not(target_arch = "wasm32"))]
    if example::sdf_collisions_demo::benchmark_requested() {
        print_benchmark();
    } else {
        start();
    }
}
