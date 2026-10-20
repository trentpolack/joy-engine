// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Browser backends: `localStorage` access for [`Storage`](crate::Storage), and downloads for
//! [`save_text_file`](crate::save_text_file) and [`save_binary_file`](crate::save_binary_file). Every call tolerates
//! blocked storage (private windows, disabled site data) and reports it instead of throwing.

use wasm_bindgen::JsCast;

use crate::StorageError;

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

/// Offer `contents` as a downloaded file named `file_name`: a UTF-8 text `Blob` behind a temporary object URL, which
/// avoids URL length limits for large exports.
pub(crate) fn download_text(file_name: &str, contents: &str) -> Result<(), String> {
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(contents));
    download_blob(file_name, &parts, "text/plain;charset=utf-8")
}

/// Offer `bytes` as a downloaded binary file named `file_name` (GLB exports, images).
pub(crate) fn download_bytes(file_name: &str, bytes: &[u8], media_type: &str) -> Result<(), String> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    download_blob(file_name, &parts, media_type)
}

/// Download a `Blob` of `parts` through a temporary object URL. The URL is revoked right after the link is clicked,
/// which every major browser allows for same-document downloads.
fn download_blob(file_name: &str, parts: &js_sys::Array, media_type: &str) -> Result<(), String> {
    let describe = |error: wasm_bindgen::JsValue| error.as_string().unwrap_or_else(|| format!("{error:?}"));
    let document = web_sys::window().and_then(|window| window.document()).ok_or("the page has no document")?;
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(media_type);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(parts, &options).map_err(describe)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(describe)?;
    let clicked = (|| -> Result<(), String> {
        let link = document.create_element("a").map_err(describe)?;
        link.set_attribute("href", &url).map_err(describe)?;
        link.set_attribute("download", file_name).map_err(describe)?;
        let link: web_sys::HtmlElement = link.dyn_into().map_err(|_| "links are not HTML elements".to_owned())?;
        link.click();
        Ok(())
    })();
    let _ = web_sys::Url::revoke_object_url(&url);
    clicked
}
