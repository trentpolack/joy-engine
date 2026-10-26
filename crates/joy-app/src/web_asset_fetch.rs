// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`AssetFetcher`] for browsers: fetches files over HTTP relative to a URL prefix, so a page downloads streaming
//! assets (such as terrain package tiles) only when they are needed. Native builds read the same relative paths from a
//! folder instead (`asset_fetch.rs`), with the same API.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// One finished fetch: the requested relative path and its bytes or a failure message.
type FetchResult = (String, Result<Vec<u8>, String>);

/// Fetches asset files relative to a URL prefix without blocking. Each request runs as its own browser fetch.
///
/// Dropping the fetcher abandons fetches in flight; their results are discarded when they arrive.
pub struct AssetFetcher {
    root: String,
    finished: Rc<RefCell<Vec<FetchResult>>>,
}

impl AssetFetcher {
    /// A fetcher reading URLs under `root` (relative to the page, such as `"assets/terrain"`; natively, a folder).
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            root: root.into().trim_end_matches('/').to_owned(),
            finished: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Start fetching `path`, relative to the root with forward slashes. Paths that leave the root fail on the next
    /// poll.
    pub fn request(&mut self, path: &str) {
        if path.is_empty() || path.starts_with('/') || path.split('/').any(|part| part == "..") {
            self.finished
                .borrow_mut()
                .push((path.to_owned(), Err("asset paths must stay inside the asset root".into())));
            return;
        }
        let url = if self.root.is_empty() { path.to_owned() } else { format!("{}/{path}", self.root) };
        let finished = Rc::downgrade(&self.finished);
        let path = path.to_owned();
        wasm_bindgen_futures::spawn_local(async move {
            let result = fetch_bytes(&url).await;
            if let Some(finished) = finished.upgrade() {
                finished.borrow_mut().push((path, result));
            }
        });
    }

    /// Fetches finished since the last poll.
    pub fn poll(&mut self) -> Vec<FetchResult> {
        std::mem::take(&mut *self.finished.borrow_mut())
    }
}

impl joy_terrain::TerrainByteSource for AssetFetcher {
    fn request(&mut self, path: &str) {
        AssetFetcher::request(self, path);
    }

    fn poll(&mut self) -> Vec<FetchResult> {
        AssetFetcher::poll(self)
    }
}

/// Fetch one URL's body, failing on network errors and unsuccessful HTTP statuses.
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no browser window")?;
    let response = JsFuture::from(window.fetch_with_str(url)).await.map_err(|error| format!("{url}: {error:?}"))?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| format!("{url}: not an HTTP response"))?;
    if !response.ok() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    let body = response.array_buffer().map_err(|error| format!("{url}: {error:?}"))?;
    let buffer = JsFuture::from(body).await.map_err(|error| format!("{url}: {error:?}"))?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
