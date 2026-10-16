// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The native desktop host: the Rust replacement for the Electron launcher (`workspace.cjs`, `development.cjs`, and the
//! preload bridge). There is no browser process or development server: the binary finds its hosting workspace, opens
//! project files in process ([`crate::server`]), and runs the editor in a native window.

#[cfg(not(target_arch = "wasm32"))]
pub mod workspace;
