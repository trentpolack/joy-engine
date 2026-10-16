// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Working-draft recovery through `runtime::Storage`: blocked or
//! failing storage reads as an absent draft and never interrupts editing.

use joy_engine::runtime::Storage;

/// Read a local draft, treating unavailable storage as no draft.
pub fn read_stored_text(storage: &Storage, key: &str) -> Option<String> {
    storage.load(key).ok().flatten()
}

/// Persist a local draft; returns whether storage accepted it.
pub fn store_text(storage: &Storage, key: &str, value: &str) -> bool {
    storage.save(key, value).is_ok()
}
