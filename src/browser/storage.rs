// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! `localStorage` access for `runtime::Storage`. Every call tolerates blocked storage (private windows, disabled site
//! data) and reports it instead of throwing.

use crate::runtime::StorageError;

/// The page origin's `localStorage`, or why it cannot be used.
fn local_storage() -> Result<web_sys::Storage, StorageError> {
    let window = web_sys::window().ok_or_else(|| StorageError::Unavailable("no browser window".to_owned()))?;
    match window.local_storage() {
        Ok(Some(storage)) => Ok(storage),
        Ok(None) => Err(StorageError::Unavailable("localStorage is not available".to_owned())),
        Err(error) => Err(StorageError::Unavailable(describe(&error))),
    }
}

/// Read a value.
pub(crate) fn load(key: &str) -> Result<Option<String>, StorageError> {
    local_storage()?
        .get_item(key)
        .map_err(|error| StorageError::Read { key: key.to_owned(), message: describe(&error) })
}

/// Write a value.
pub(crate) fn save(key: &str, text: &str) -> Result<(), StorageError> {
    local_storage()?
        .set_item(key, text)
        .map_err(|error| StorageError::Write { key: key.to_owned(), message: describe(&error) })
}

/// Delete a value.
pub(crate) fn remove(key: &str) -> Result<(), StorageError> {
    local_storage()?
        .remove_item(key)
        .map_err(|error| StorageError::Write { key: key.to_owned(), message: describe(&error) })
}

/// A readable description of a thrown JavaScript value (for example a `QuotaExceededError`).
fn describe(error: &wasm_bindgen::JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}
