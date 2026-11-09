// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The cook's content-addressed cache: one cooked geometry record per [`CookKey`], so unchanged assets never evaluate
//! twice, across rebuilds and across profiles and targets (a browser build reuses what a native release build cooked).
//!
//! A record's file name is its key, and the key hashes every evaluation input including the FORM evaluator version, so
//! entries never go stale: an edit produces a new key and the old file is simply no longer read. Files are written to a
//! temporary sibling and renamed into place, so parallel builds sharing the folder never read a partial record. A file
//! that does not decode is ignored and rewritten. Clearing the folder (or `cargo clean`, when it lives in the target
//! directory) only costs re-evaluation.

use std::io;
use std::path::{
    Path,
    PathBuf,
};

use joy_form::{
    CookKey,
    decode_geometry,
};

/// File extension of cached records.
const RECORD_EXTENSION: &str = "jfgm";

/// A folder of cooked geometry records keyed by content hash. Owns nothing but the folder path; the folder is created
/// on the first store.
#[derive(Clone, Debug)]
pub struct CookCache {
    directory: PathBuf,
}

impl CookCache {
    /// A cache in `directory`, which need not exist yet.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// The cache folder.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The record cached for `key`, when one exists and decodes.
    pub fn load(&self, key: CookKey) -> Option<Vec<u8>> {
        let record = std::fs::read(self.record_path(key)).ok()?;
        decode_geometry(&record).is_ok().then_some(record)
    }

    /// Cache `record` for `key`, replacing any earlier file atomically.
    pub fn store(&self, key: CookKey, record: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let path = self.record_path(key);
        let temporary = path.with_extension(format!("{RECORD_EXTENSION}.{}.tmp", std::process::id()));
        std::fs::write(&temporary, record)?;
        std::fs::rename(&temporary, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
    }

    /// Where `key`'s record lives.
    pub fn record_path(&self, key: CookKey) -> PathBuf {
        self.directory.join(format!("{}.{RECORD_EXTENSION}", key.to_hex()))
    }
}
