// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`AssetFetcher`] for native builds: reads files under a root folder on a worker thread, so streaming assets (such as
//! terrain package tiles) never block a frame. The browser build fetches the same relative paths over HTTP instead
//! (`web_asset_fetch.rs`), with the same API.

use std::path::{
    Path,
    PathBuf,
};
use std::sync::mpsc::{
    Receiver,
    Sender,
    channel,
};
use std::thread::JoinHandle;

/// One finished read: the requested relative path and its bytes or a failure message.
type FetchResult = (String, Result<Vec<u8>, String>);

/// Reads asset files relative to a root without blocking. Requests finish in order on one worker thread.
///
/// Dropping the fetcher stops the worker after the read in progress; reads still queued are abandoned.
pub struct AssetFetcher {
    // The worker and the channels to it; the request sender drops first so the worker sees the end of its queue.
    requests: Option<Sender<String>>,
    results: Receiver<FetchResult>,
    worker: Option<JoinHandle<()>>,
    rejected: Vec<FetchResult>,
}

impl AssetFetcher {
    /// A fetcher reading files under `root` (a folder; in browsers, a URL prefix relative to the page).
    pub fn new(root: impl Into<String>) -> Self {
        let root = PathBuf::from(root.into());
        let (requests, queue) = channel::<String>();
        let (finished, results) = channel::<FetchResult>();
        let worker = std::thread::Builder::new()
            .name("joy asset fetch".into())
            .spawn(move || {
                for path in queue {
                    let bytes = std::fs::read(root.join(&path)).map_err(|error| error.to_string());
                    if finished.send((path, bytes)).is_err() {
                        return;
                    }
                }
            })
            .ok();
        Self {
            requests: worker.as_ref().map(|_| requests),
            results,
            worker,
            rejected: Vec::new(),
        }
    }

    /// Start reading `path`, relative to the root with forward slashes. Paths that leave the root fail on the next
    /// poll.
    pub fn request(&mut self, path: &str) {
        if !is_relative_inside(path) {
            self.rejected.push((path.to_owned(), Err("asset paths must stay inside the asset root".into())));
            return;
        }
        let sent = self.requests.as_ref().is_some_and(|requests| requests.send(path.to_owned()).is_ok());
        if !sent {
            self.rejected.push((path.to_owned(), Err("the asset reader is not running".into())));
        }
    }

    /// Reads finished since the last poll.
    pub fn poll(&mut self) -> Vec<FetchResult> {
        let mut finished = std::mem::take(&mut self.rejected);
        finished.extend(self.results.try_iter());
        finished
    }
}

impl Drop for AssetFetcher {
    fn drop(&mut self) {
        self.requests = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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

/// Whether a request path is relative and never climbs above the root.
fn is_relative_inside(path: &str) -> bool {
    !path.is_empty()
        && !Path::new(path).is_absolute()
        && !path.starts_with('/')
        && !path.split(['/', '\\']).any(|part| part == "..")
}
