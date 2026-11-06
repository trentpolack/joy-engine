// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Adapt libc_alloc's allocation entry point to WASI libc; mlua builds vendored Lua itself.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build/wasi_allocator.c");
    if env::var("TARGET").expect("Cargo supplies TARGET") == "wasm32-unknown-unknown" {
        cc::Build::new().file("build/wasi_allocator.c").compile("joy_wasi_allocator");
    }
}
