// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Game assets that ship embedded and hot reload in native development builds.
//!
//! Games embed their authored text assets (`.form`, `.forminst`, `.joyfx`, `.joylevel`, `.joyobject`, tuning JSON) with
//! [`game_assets!`](crate::game_assets), so browser and release builds need no file system. In native development
//! builds the same table also remembers the game's source folder: [`GameAssets::read`] then prefers the file on disk,
//! and [`GameAssets::poll_changes`] reports which previously read files changed, so a game can rebuild only what an
//! edit touched (a FORM asset recompiles, an effect restarts, a level reloads) without restarting or recompiling.
//!
//! Paths are the project-relative `assets/…` paths levels and components already use. Reads outside the embedded table
//! or the source folder's `assets/` directory are refused, so a hot-reload path can never reach other files.
//!
//! Hot reload reads any `assets/` file from disk, so a file a game forgot to embed would work in development and fail
//! only in browser and release builds. Development reads of a path the table does not embed therefore succeed but log
//! a warning once per path, and [`GameAssets::unembedded_reads`] lists them.
//!
//! The table lives in the shell crate rather than the development tools because release and browser builds need it as
//! much as development builds do.

use std::collections::{
    BTreeSet,
    HashMap,
};
use std::path::{
    Path,
    PathBuf,
};
use std::time::SystemTime;

/// Seconds between file checks while hot reloading.
pub const ASSET_POLL_SECONDS: f64 = 0.5;

/// Folder every game asset path starts with.
const ASSET_FOLDER: &str = "assets/";

/// One embedded asset: its project-relative path and text.
pub type EmbeddedAsset = (&'static str, &'static str);

/// A file's modification time and length, enough to notice edits cheaply.
type FileStamp = Option<(SystemTime, u64)>;

/// Why an asset could not be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AssetReadError {
    /// The path is not a plain `assets/…` path (absolute, parent segments, or another folder).
    #[error("{0} is not a project assets/ path.")]
    OutsideAssets(String),
    /// Neither the embedded table nor the source folder has the asset.
    #[error("{0} is not embedded in this build.")]
    Missing(String),
}

/// A game's embedded assets, with native development hot reload from its source folder.
///
/// Owns only the embedded table, the optional source folder, and file stamps of what it has read; it never holds
/// asset text it did not embed. Independent instances do not share state.
#[derive(Debug)]
pub struct GameAssets {
    // Text compiled into the build, by project-relative path.
    embedded: HashMap<String, &'static str>,

    // Hot reload: the project folder files are read from (native builds only), and each read file's stamp.
    source_root: Option<PathBuf>,
    stamps: HashMap<String, FileStamp>,

    // Paths read from the source folder that the table does not embed, each warned about once.
    unembedded: BTreeSet<String>,

    // Seconds accumulated toward the next file check.
    poll_elapsed: f64,
}

impl GameAssets {
    /// A table of `embedded` assets (project-relative path and text); `source_root` (the game's project folder) enables
    /// hot reload where files can be read (native builds; browsers ignore it). Use [`game_assets!`](crate::game_assets),
    /// which embeds the files and passes the source folder only in native development builds, or
    /// [`source_root`](crate::source_root) to build the table from text a game already embeds.
    pub fn new<P: Into<String>>(
        embedded: impl IntoIterator<Item = (P, &'static str)>,
        source_root: Option<&str>,
    ) -> Self {
        Self {
            embedded: embedded.into_iter().map(|(path, text)| (path.into(), text)).collect(),
            source_root: source_root.filter(|_| FILES_READABLE).map(PathBuf::from),
            stamps: HashMap::new(),
            unembedded: BTreeSet::new(),
            poll_elapsed: 0.0,
        }
    }

    /// Whether reads come from the source folder and [`poll_changes`](Self::poll_changes) can report edits.
    pub fn hot_reload(&self) -> bool {
        self.source_root.is_some()
    }

    /// The current text of `path`: the source folder's file while hot reloading (falling back to the embedded text
    /// when the file cannot be read), otherwise the embedded text. A hot-reload read of a file the table does not embed
    /// succeeds but warns once, since browser and release builds will report it [`Missing`](AssetReadError::Missing).
    pub fn read(&mut self, path: &str) -> Result<String, AssetReadError> {
        if !is_asset_path(path) {
            return Err(AssetReadError::OutsideAssets(path.to_owned()));
        }
        if let Some(root) = &self.source_root {
            let file = root.join(path);
            self.stamps.insert(path.to_owned(), file_stamp(&file));
            if let Some(text) = read_file(&file) {
                self.note_unembedded(path);
                return Ok(text);
            }
        }
        self.embedded.get(path).map(|text| (*text).to_owned()).ok_or_else(|| AssetReadError::Missing(path.to_owned()))
    }

    /// Paths read from the source folder that this table does not embed, in sorted order: each works in development
    /// and is missing from browser and release builds. Always empty without hot reload.
    pub fn unembedded_reads(&self) -> impl Iterator<Item = &str> {
        self.unembedded.iter().map(String::as_str)
    }

    /// The text compiled into the build for `path`, ignoring the source folder.
    pub fn embedded(&self, path: &str) -> Option<&'static str> {
        self.embedded.get(path).copied()
    }

    /// Advance the poll clock by `elapsed_seconds` and, at most every [`ASSET_POLL_SECONDS`], return the previously read
    /// paths whose files changed since they were last read, in sorted order. Always empty without hot reload.
    ///
    /// Reported paths stay reported until the game reads them again, so a failed rebuild is retried on the next poll.
    pub fn poll_changes(&mut self, elapsed_seconds: f64) -> Vec<String> {
        let Some(root) = &self.source_root else {
            return Vec::new();
        };
        if elapsed_seconds.is_finite() {
            self.poll_elapsed += elapsed_seconds.max(0.0);
        }
        if self.poll_elapsed < ASSET_POLL_SECONDS {
            return Vec::new();
        }
        self.poll_elapsed = 0.0;
        let mut changed: Vec<String> = self
            .stamps
            .iter()
            .filter(|(path, stamp)| file_stamp(&root.join(path.as_str())) != **stamp)
            .map(|(path, _)| path.clone())
            .collect();
        changed.sort();
        changed
    }
}

impl GameAssets {
    /// Record and warn once when a development read reaches a file the release build would not embed.
    fn note_unembedded(&mut self, path: &str) {
        if self.embedded.contains_key(path) || self.unembedded.contains(path) {
            return;
        }
        log::warn!(
            "{path} was read from the project folder but is not embedded, so browser and release builds will not find \
             it; add it to the game's game_assets! list (or its GameAssets table)."
        );
        self.unembedded.insert(path.to_owned());
    }
}

/// Whether `path` is a plain relative `assets/…` path without parent, current, or empty segments.
fn is_asset_path(path: &str) -> bool {
    path.starts_with(ASSET_FOLDER)
        && !path.contains('\\')
        && path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

// Browsers have no project folder: their builds read only embedded text.
#[cfg(not(target_arch = "wasm32"))]
const FILES_READABLE: bool = true;
#[cfg(target_arch = "wasm32")]
const FILES_READABLE: bool = false;

/// Largest asset hot reload reads from disk; larger files fall back to their embedded text.
#[cfg(not(target_arch = "wasm32"))]
const MAXIMUM_FILE_BYTES: u64 = 1024 * 1024;

/// Modification time and length, enough to notice outside edits cheaply.
#[cfg(not(target_arch = "wasm32"))]
fn file_stamp(path: &Path) -> FileStamp {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

#[cfg(target_arch = "wasm32")]
fn file_stamp(_path: &Path) -> FileStamp {
    None
}

/// The text of a regular file of at most [`MAXIMUM_FILE_BYTES`].
#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

#[cfg(target_arch = "wasm32")]
fn read_file(_path: &Path) -> Option<String> {
    None
}

/// Embed a game's text assets and enable native development hot reload from the game's project folder.
///
/// ```ignore
/// let assets = joy_engine::game_assets!["assets/main.joylevel", "assets/form/tower.form"];
/// ```
///
/// Paths are relative to the calling crate's manifest directory and must start with `assets/`. Browser and release
/// builds read only the embedded text.
#[macro_export]
macro_rules! game_assets {
    ($($path:literal),* $(,)?) => {
        $crate::GameAssets::new(
            [$(($path, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/", $path)))),*],
            $crate::source_root!(),
        )
    };
}

/// The calling crate's project folder in native development builds, and `None` otherwise: the `source_root` argument
/// of [`GameAssets::new`] for games that build their asset table from text they already embed.
#[macro_export]
macro_rules! source_root {
    () => {
        if cfg!(all(debug_assertions, not(target_arch = "wasm32"))) { Some(env!("CARGO_MANIFEST_DIR")) } else { None }
    };
}

#[cfg(test)]
#[path = "game_assets.test.rs"]
mod tests;
