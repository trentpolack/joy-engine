// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Persistent text storage for small saves such as character progression: the page origin's `localStorage` in
//! browsers, and one file per key in a per-user data folder natively.
//!
//! This is the Rust counterpart of games calling `localStorage.getItem`/`setItem` directly in TypeScript. Keys are
//! plain names (`joy.luna.character.v1`); values are text the caller serializes and validates. Reads and writes are
//! synchronous and small (at most 1 MiB per value). Failures never panic: an unavailable store (a private browsing
//! window that blocks storage, a read-only disk, no home folder) reports [`StorageError`] so the game can keep playing
//! and tell the player progress will not persist.
//!
//! Native values live in `<data>/joy-games/<application>/<key>`, where `<data>` is `%APPDATA%` on Windows,
//! `~/Library/Application Support` on macOS, and `$XDG_DATA_HOME` (or `~/.local/share`) elsewhere. Writes replace the
//! file atomically, so an interrupted save leaves the previous value intact.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// Largest value accepted by [`Storage::save`] and read by [`Storage::load`].
pub const MAXIMUM_STORAGE_BYTES: usize = 1024 * 1024;

/// Folder under the platform data directory that holds every Joy game's saves.
#[cfg(not(target_arch = "wasm32"))]
const DATA_FOLDER: &str = "joy-games";

/// Why a storage operation did not complete.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    /// The platform store cannot be used (blocked browser storage or no writable data folder).
    #[error("Storage is unavailable: {0}")]
    Unavailable(String),
    /// Keys are 1 to 128 ASCII letters, digits, periods, hyphens, or underscores, not starting with a period.
    #[error("Storage key \"{0}\" must be 1 to 128 letters, digits, periods, hyphens, or underscores.")]
    InvalidKey(String),
    /// The value exceeds [`MAXIMUM_STORAGE_BYTES`].
    #[error("Storage value for \"{0}\" exceeds 1 MiB.")]
    TooLarge(String),
    /// Reading the stored value failed.
    #[error("Could not read \"{key}\": {message}")]
    Read {
        /// The key being read.
        key: String,
        /// Platform detail.
        message: String,
    },
    /// Writing the value failed; the previous value is unchanged.
    #[error("Could not save \"{key}\": {message}")]
    Write {
        /// The key being written.
        key: String,
        /// Platform detail.
        message: String,
    },
}

/// A handle to one application's persistent text values. Cheap to create; holds no open files.
#[derive(Clone, Debug)]
pub struct Storage {
    #[cfg(not(target_arch = "wasm32"))]
    directory: Result<PathBuf, String>,
}

impl Storage {
    /// Storage for `application` (a short folder name such as `luna`). Browsers share the page origin's
    /// `localStorage`, so keys should already be namespaced (`joy.luna.character.v1`).
    pub fn new(application: &str) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let directory = if valid_key(application) {
                data_directory().map(|data| data.join(DATA_FOLDER).join(application))
            } else {
                Err(format!("\"{application}\" is not a valid application folder name"))
            };
            Self { directory }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = application;
            Self {}
        }
    }

    /// Native storage in an explicit folder, for tools and tests. The folder is created on the first save.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn in_directory(directory: impl Into<PathBuf>) -> Self {
        Self { directory: Ok(directory.into()) }
    }

    /// The stored text for `key`, or `None` when nothing was saved.
    pub fn load(&self, key: &str) -> Result<Option<String>, StorageError> {
        if !valid_key(key) {
            return Err(StorageError::InvalidKey(key.to_owned()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.directory.as_ref().map_err(|message| StorageError::Unavailable(message.clone()))?.join(key);
            let read_error = |message: String| StorageError::Read { key: key.to_owned(), message };
            match std::fs::metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(read_error(error.to_string())),
                Ok(metadata) if !metadata.is_file() => return Err(read_error("not a regular file".to_owned())),
                Ok(metadata) if metadata.len() > MAXIMUM_STORAGE_BYTES as u64 => {
                    return Err(read_error("the stored value exceeds 1 MiB".to_owned()));
                }
                Ok(_) => {}
            }
            std::fs::read_to_string(&path).map(Some).map_err(|error| read_error(error.to_string()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::browser::storage::load(key)
        }
    }

    /// Store `text` under `key`, replacing any previous value.
    pub fn save(&self, key: &str, text: &str) -> Result<(), StorageError> {
        if !valid_key(key) {
            return Err(StorageError::InvalidKey(key.to_owned()));
        }
        if text.len() > MAXIMUM_STORAGE_BYTES {
            return Err(StorageError::TooLarge(key.to_owned()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let directory = self.directory.as_ref().map_err(|message| StorageError::Unavailable(message.clone()))?;
            write_atomically(directory, key, text)
                .map_err(|error| StorageError::Write { key: key.to_owned(), message: error.to_string() })
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::browser::storage::save(key, text)
        }
    }

    /// Delete the value for `key`; deleting a missing value succeeds.
    pub fn remove(&self, key: &str) -> Result<(), StorageError> {
        if !valid_key(key) {
            return Err(StorageError::InvalidKey(key.to_owned()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.directory.as_ref().map_err(|message| StorageError::Unavailable(message.clone()))?.join(key);
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(StorageError::Write { key: key.to_owned(), message: error.to_string() }),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::browser::storage::remove(key)
        }
    }
}

/// Whether `key` is a safe, portable name for a browser key and a file.
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && !key.starts_with('.')
        && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// The per-user data folder for this platform.
#[cfg(not(target_arch = "wasm32"))]
fn data_directory() -> Result<PathBuf, String> {
    let variable = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let directory = if cfg!(target_os = "windows") {
        variable("APPDATA")
    } else if cfg!(target_os = "macos") {
        variable("HOME").map(|home| home.join("Library").join("Application Support"))
    } else {
        variable("XDG_DATA_HOME").or_else(|| variable("HOME").map(|home| home.join(".local").join("share")))
    };
    directory.ok_or_else(|| "no per-user data folder is configured".to_owned())
}

/// Write through a temporary file and rename it over the destination.
#[cfg(not(target_arch = "wasm32"))]
fn write_atomically(directory: &std::path::Path, key: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".{key}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, directory.join(key))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("joy-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn values_round_trip_replace_and_remove() {
        let directory = scratch("round-trip");
        let storage = Storage::in_directory(&directory);
        assert_eq!(storage.load("joy.test.save.v1"), Ok(None));
        storage.save("joy.test.save.v1", "{\"version\":1}").unwrap();
        assert_eq!(storage.load("joy.test.save.v1").unwrap().as_deref(), Some("{\"version\":1}"));
        storage.save("joy.test.save.v1", "second").unwrap();
        assert_eq!(storage.load("joy.test.save.v1").unwrap().as_deref(), Some("second"));
        let leftovers = std::fs::read_dir(&directory).unwrap().filter(|entry| entry.is_ok()).count();
        assert_eq!(leftovers, 1, "atomic writes leave no temporary files");
        storage.remove("joy.test.save.v1").unwrap();
        storage.remove("joy.test.save.v1").unwrap();
        assert_eq!(storage.load("joy.test.save.v1"), Ok(None));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unsafe_keys_oversized_values_and_missing_folders_are_reported() {
        let storage = Storage::in_directory(scratch("keys"));
        for key in ["", "../escape", ".hidden", "a/b", "spaces are bad", &"k".repeat(129)] {
            assert_eq!(storage.load(key), Err(StorageError::InvalidKey(key.to_owned())));
            assert!(matches!(storage.save(key, "x"), Err(StorageError::InvalidKey(_))));
        }
        let large = "x".repeat(MAXIMUM_STORAGE_BYTES + 1);
        assert_eq!(storage.save("large", &large), Err(StorageError::TooLarge("large".to_owned())));

        let unavailable = Storage { directory: Err("no home".to_owned()) };
        assert_eq!(unavailable.load("key"), Err(StorageError::Unavailable("no home".to_owned())));
        assert!(matches!(Storage::new("../escape").save("key", "x"), Err(StorageError::Unavailable(_))));
    }
}
