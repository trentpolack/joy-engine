// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native persistence for the post-processing editor. The profile lives under `postProcessing` in a JSON document (a
//! level or project file); saves replace only that key, write atomically, and refuse to overwrite a file that changed
//! since it was read, like the TypeScript development server plugin.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde_json::Value;
use web_time::Instant;

use crate::core::PostProcessingProfile;
use crate::development::files::{WriteError, pretty_json, read_document, revision_of, stamp, write_atomically};
use crate::development::post_processing_session::profile_json;

/// How often the file is checked for outside edits.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A validated profile and the revision of the document it came from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) profile: Value,
    pub(crate) revision: String,
}

/// Why a save did not complete.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SaveError {
    /// The file changed since the session's revision; the snapshot is what is on disk now.
    Conflict(Snapshot, String),
    /// The file could not be read, validated, or written.
    Failed(String),
}

/// One document on disk and the modification stamp last seen.
pub(crate) struct ProfileFile {
    path: PathBuf,
    stamp: Option<(SystemTime, u64)>,
    last_poll: Option<Instant>,
}

impl ProfileFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, stamp: None, last_poll: None }
    }

    /// Read and validate the current profile.
    pub(crate) fn read(&mut self) -> Result<Snapshot, String> {
        let text = read_document(&self.path)?;
        self.stamp = stamp(&self.path);
        snapshot(&text)
    }

    /// Report the profile when the file changed since the last read or poll; checks at most twice a second.
    pub(crate) fn poll(&mut self) -> Option<Result<Snapshot, String>> {
        let now = Instant::now();
        if self.last_poll.is_some_and(|last| now.duration_since(last) < POLL_INTERVAL) {
            return None;
        }
        self.last_poll = Some(now);
        let current = stamp(&self.path);
        if current == self.stamp {
            return None;
        }
        Some(self.read())
    }

    /// Write `profile` into the document when it is still at `revision`; returns the new revision.
    pub(crate) fn save(&mut self, profile: &PostProcessingProfile, revision: &str) -> Result<String, SaveError> {
        let current_text = read_document(&self.path).map_err(SaveError::Failed)?;
        let current = snapshot(&current_text).map_err(SaveError::Failed)?;
        if current.revision != revision {
            let message = "The file changed. Choose Use Disk or Keep Local before saving.".to_owned();
            return Err(SaveError::Conflict(current, message));
        }
        let mut document: Value =
            serde_json::from_str(&current_text).map_err(|error| SaveError::Failed(error.to_string()))?;
        let Some(record) = document.as_object_mut() else {
            return Err(SaveError::Failed("The document must be a JSON object.".to_owned()));
        };
        record.insert("postProcessing".to_owned(), profile_json(profile));
        let text = pretty_json(&document).map_err(SaveError::Failed)?;
        let saved = match write_atomically(&self.path, &text, revision) {
            Ok(saved) => saved,
            Err(WriteError::Changed) => {
                let latest =
                    read_document(&self.path).and_then(|latest| snapshot(&latest)).map_err(SaveError::Failed)?;
                return Err(SaveError::Conflict(latest, "The file changed during saving.".to_owned()));
            }
            Err(WriteError::Failed(message)) => return Err(SaveError::Failed(message)),
        };
        self.stamp = stamp(&self.path);
        Ok(saved)
    }
}

/// The validated `postProcessing` profile of a document's text.
fn snapshot(text: &str) -> Result<Snapshot, String> {
    let document: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let profile = PostProcessingProfile::validate(document.get("postProcessing").unwrap_or(&Value::Null))
        .map_err(|error| error.to_string())?;
    Ok(Snapshot { profile: profile_json(&profile), revision: revision_of(text) })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use serde_json::json;

    fn document(path: &Path, exposure: f64) {
        let text = serde_json::to_string_pretty(&json!({
            "name": "Test Level",
            "postProcessing": { "version": 1, "preset": "default", "overrides": { "exposure": exposure } },
            "objects": []
        }))
        .unwrap();
        fs::write(path, format!("{text}\n")).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("joy-profile-{}-{name}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        directory.join("level.joylevel")
    }

    #[test]
    fn saves_replace_only_the_profile_and_detect_outside_edits() {
        let path = scratch("save");
        document(&path, 2.0);
        let mut file = ProfileFile::new(path.clone());
        let loaded = file.read().unwrap();
        assert_eq!(loaded.profile["overrides"]["exposure"], json!(2.0));

        let edited =
            PostProcessingProfile::validate(&json!({ "version": 1, "preset": "soft", "overrides": {} })).unwrap();
        let revision = file.save(&edited, &loaded.revision).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("}\n"));
        let saved: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["name"], "Test Level");
        assert_eq!(saved["postProcessing"]["preset"], "soft");
        assert_eq!(saved.as_object().unwrap().keys().collect::<Vec<_>>(), ["name", "postProcessing", "objects"]);
        assert!(file.poll().is_none(), "the editor's own save is not an outside edit");

        // A stale revision is refused with what is on disk now.
        document(&path, 3.0);
        match file.save(&edited, &revision) {
            Err(SaveError::Conflict(snapshot, _)) => assert_eq!(snapshot.profile["overrides"]["exposure"], json!(3.0)),
            other => panic!("expected a conflict, got {other:?}"),
        }
        let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary files remain");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn invalid_documents_are_reported() {
        let path = scratch("invalid");
        fs::write(&path, "{\"postProcessing\": {\"version\": 2}}").unwrap();
        assert!(ProfileFile::new(path.clone()).read().is_err());
        fs::write(&path, "not json").unwrap();
        assert!(ProfileFile::new(path.clone()).read().is_err());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(ProfileFile::new(path).read().is_err());
    }
}
