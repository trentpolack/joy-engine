// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native file helpers shared by development editors that save into authored JSON documents: bounded reads, content
//! revisions, and atomic replacement that refuses to overwrite a file changed since it was read.

use std::fs;
use std::hash::{
    DefaultHasher,
    Hasher,
};
use std::io::Write;
use std::path::{
    Path,
    PathBuf,
};
use std::time::SystemTime;

/// Largest document an editor reads or writes.
const MAXIMUM_BYTES: u64 = 1024 * 1024;

/// Why an atomic write did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WriteError {
    /// The file no longer has the expected revision.
    Changed,
    /// The file could not be read or written.
    Failed(String),
}

/// Read a regular file of at most 1 MiB.
pub(crate) fn read_document(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_BYTES {
        return Err(format!("{} must be a regular file of at most 1 MiB.", path.display()));
    }
    fs::read_to_string(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))
}

/// A content identity for a document's text.
pub(crate) fn revision_of(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(text.as_bytes());
    format!("{:016x}-{}", hasher.finish(), text.len())
}

/// Modification time and length, enough to notice outside edits cheaply.
pub(crate) fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Replace `path` with `text` through a sibling temporary file, provided the file is still at `revision` both before
/// and after the temporary is written. Returns the new revision.
pub(crate) fn write_atomically(path: &Path, text: &str, revision: &str) -> Result<String, WriteError> {
    let unchanged = || -> Result<(), WriteError> {
        let current = read_document(path).map_err(WriteError::Failed)?;
        if revision_of(&current) == revision { Ok(()) } else { Err(WriteError::Changed) }
    };
    unchanged()?;
    let temporary = temporary_path(path);
    let result = (|| -> Result<(), WriteError> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| WriteError::Failed(error.to_string()))?;
        file.write_all(text.as_bytes()).map_err(|error| WriteError::Failed(error.to_string()))?;
        file.sync_all().map_err(|error| WriteError::Failed(error.to_string()))?;
        // Check again after writing, in case another editor saved meanwhile.
        unchanged()?;
        fs::rename(&temporary, path).map_err(|error| WriteError::Failed(error.to_string()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|()| revision_of(text))
}

/// Pretty JSON with a trailing newline, as the TypeScript tools write documents.
pub(crate) fn pretty_json(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

/// A unique sibling path for an atomic replacement.
fn temporary_path(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(|| "document".into(), |name| name.to_string_lossy().into_owned());
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
    path.with_file_name(format!(".{name}.{}-{nanos}.tmp", std::process::id()))
}
