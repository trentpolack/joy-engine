// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native entry point for PARTICLE LAB. Browsers start through the library's `web_main!` entry instead.

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = particle_lab::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
