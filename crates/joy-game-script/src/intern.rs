// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared `'static` text for records read from data modules.
//!
//! Games keep authored catalog records (parts, weapons, archetypes) as `&'static str` so every system can hold them by
//! reference, but data modules are read at run time and hot reloaded in development. [`intern`] gives each distinct
//! text one leaked allocation for the life of the process: reloading a module leaks only text that changed, and
//! browser and release builds read their data once. Use it for authored vocabulary, never for unbounded run-time text.

use std::collections::BTreeSet;
use std::sync::Mutex;

/// Every interned text, for reuse across loads.
static INTERNED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

/// The shared `'static` copy of `text`, allocating it the first time it is seen.
pub fn intern(text: &str) -> &'static str {
    // A panic while holding the table cannot leave it inconsistent (inserts are atomic), so a poisoned lock is safe.
    let mut interned = INTERNED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = interned.get(text) {
        return existing;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    interned.insert(leaked);
    leaked
}
