// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Persistent saves and player-facing file exports.
//!
//! [`Storage`] keeps small text values such as character progression across sessions (the page origin's
//! `localStorage` in browsers, one file per key in a per-user data folder natively), and [`SaveSlot`] guards one save
//! against overlapping writes. [`save_text_file`] and [`save_binary_file`] hand a generated file to the player: a
//! browser download, or a file in the working directory natively. Failures never panic; they report typed errors so a
//! game can keep playing.

mod save_file;
mod save_slot;
mod storage;
#[cfg(target_arch = "wasm32")]
mod web;

pub use save_file::{
    SaveFileError,
    save_binary_file,
    save_text_file,
};
pub use save_slot::{
    SaveGuard,
    SaveSlot,
    SaveWrite,
};
pub use storage::{
    MAXIMUM_STORAGE_BYTES,
    Storage,
    StorageError,
};
