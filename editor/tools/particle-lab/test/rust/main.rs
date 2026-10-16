// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! PARTICLE LAB's Rust integration tests, one binary: the TypeScript `document`, `examples`, and `texture-lifecycle`
//! tests ported, session behavior (Live, Apply, drafts, undo, playback), and headless runs of the real interface with
//! synthetic input. GPU tests skip with a note when no adapter exists; set `JOY_RENDER_CAPTURE_DIR` to keep captures.
#![cfg(not(target_arch = "wasm32"))]

mod document;
mod examples;
mod interface;
mod session;
mod support;
mod texture_lifecycle;
