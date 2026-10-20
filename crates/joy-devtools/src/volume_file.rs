// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native persistence for the post-processing editor. The edited settings live in one entity's `postProcess`
//! component inside a level document; saves replace only that component's settings (its volume fields and the rest of
//! the document are untouched), write atomically, and refuse to overwrite a file that changed since it was read.

use std::path::PathBuf;
use std::time::{
    Duration,
    SystemTime,
};

use serde_json::Value;
use web_time::Instant;

use crate::{
    files::WriteError,
    files::pretty_json,
    files::read_document,
    files::revision_of,
    files::stamp,
    files::write_atomically,
};
use joy_gameplay::component_type;
use joy_renderer::PostProcessOverrides;
use joy_renderer::post_process::post_process_control;
use joy_scene::PostProcessComponent;

/// How often the file is checked for outside edits.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A volume's validated settings and the revision of the document they came from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) overrides: PostProcessOverrides,
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

/// One level document on disk, the volume entity edited in it, and the modification stamp last seen.
pub(crate) struct VolumeFile {
    path: PathBuf,
    entity: String,
    stamp: Option<(SystemTime, u64)>,
    last_poll: Option<Instant>,
}

impl VolumeFile {
    pub(crate) fn new(path: PathBuf, entity: impl Into<String>) -> Self {
        Self {
            path,
            entity: entity.into(),
            stamp: None,
            last_poll: None,
        }
    }

    /// Read and validate the volume's current settings.
    pub(crate) fn read(&mut self) -> Result<Snapshot, String> {
        let text = read_document(&self.path)?;
        self.stamp = stamp(&self.path);
        snapshot(&text, &self.entity)
    }

    /// Report the settings when the file changed since the last read or poll; checks at most twice a second.
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

    /// Write `overrides` into the volume when the document is still at `revision`; returns the new revision.
    pub(crate) fn save(&mut self, overrides: &PostProcessOverrides, revision: &str) -> Result<String, SaveError> {
        let current_text = read_document(&self.path).map_err(SaveError::Failed)?;
        let current = snapshot(&current_text, &self.entity).map_err(SaveError::Failed)?;
        if current.revision != revision {
            let message = "The file changed. Choose Use Disk or Keep Local before saving.".to_owned();
            return Err(SaveError::Conflict(current, message));
        }
        let mut document: Value =
            serde_json::from_str(&current_text).map_err(|error| SaveError::Failed(error.to_string()))?;
        let record = volume_record_mut(&mut document, &self.entity).map_err(SaveError::Failed)?;

        // Keep the volume fields in place and replace every setting with the saved ones, in draft order.
        record.retain(|key, _| post_process_control(key).is_none());
        for (key, value) in overrides.record() {
            record.insert(key.clone(), value.clone());
        }

        let text = pretty_json(&document).map_err(SaveError::Failed)?;
        let saved = match write_atomically(&self.path, &text, revision) {
            Ok(saved) => saved,
            Err(WriteError::Changed) => {
                let latest = read_document(&self.path)
                    .and_then(|latest| snapshot(&latest, &self.entity))
                    .map_err(SaveError::Failed)?;
                return Err(SaveError::Conflict(latest, "The file changed during saving.".to_owned()));
            }
            Err(WriteError::Failed(message)) => return Err(SaveError::Failed(message)),
        };
        self.stamp = stamp(&self.path);
        Ok(saved)
    }
}

/// The validated settings of `entity`'s `postProcess` component in a level document's text.
fn snapshot(text: &str, entity: &str) -> Result<Snapshot, String> {
    let mut document: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let record = volume_record_mut(&mut document, entity)?;
    let component = PostProcessComponent::read(record).map_err(|error| error.to_string())?;
    Ok(Snapshot {
        overrides: component.overrides,
        revision: revision_of(text),
    })
}

/// The `postProcess` component record of `entity` in a level document.
fn volume_record_mut<'a>(
    document: &'a mut Value,
    entity: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, String> {
    let entities = document
        .get_mut("entities")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "The level document has no entities.".to_owned())?;
    entities
        .iter_mut()
        .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(entity))
        .and_then(|candidate| candidate.get_mut("components"))
        .and_then(|components| components.get_mut(component_type::POST_PROCESS))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("Entity {entity} has no postProcess component."))
}

#[cfg(test)]
#[path = "volume_file.test.rs"]
mod tests;
