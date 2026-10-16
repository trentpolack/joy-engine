// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One saved value, such as a game's progress, guarded against accidental loss.
//!
//! A failed read does not mean nothing is saved: the value may exist and simply be unreadable right now (a permission
//! problem, a file that is not text), and native storage can often still be written. A game that treated the failure
//! as an empty slot would overwrite the player's progress with a fresh start on its next autosave. A [`SaveSlot`]
//! therefore refuses ordinary writes after a failed read, and after the game [rejects](SaveSlot::reject) what it read
//! (a damaged save, or one from a newer version, kept so a fixed build can still read it), until the game explicitly
//! [replaces](SaveSlot::replace) the value, for example when the player starts a new campaign.

use crate::runtime::storage::{Storage, StorageError};

/// Why a save slot refuses ordinary writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveGuard {
    /// Reading the slot failed, so whatever it holds is unknown and may be the player's progress.
    ReadFailed,
    /// The slot held text the game could not use; it stays in place for a build that can.
    Rejected,
}

/// What an ordinary [`SaveSlot::write`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveWrite {
    /// The text was stored.
    Written,
    /// The slot already held this text, so nothing was written.
    Unchanged,
    /// The slot is guarded; nothing was written and the stored value is untouched.
    Guarded(SaveGuard),
}

/// A storage key holding one saved value, guarded so a value that could not be read or used is never overwritten by
/// an ordinary save.
#[derive(Clone, Debug)]
pub struct SaveSlot {
    storage: Storage,
    key: String,
    guard: Option<SaveGuard>,
    // The text the slot is known to hold, so unchanged saves skip the write.
    stored: Option<String>,
}

impl SaveSlot {
    /// Open `key` in `storage` and read it: the stored text, `None` when nothing is saved, or the read error. A failed
    /// read guards the slot with [`SaveGuard::ReadFailed`].
    pub fn open(storage: Storage, key: impl Into<String>) -> (Self, Result<Option<String>, StorageError>) {
        let key = key.into();
        let loaded = storage.load(&key);
        let (guard, stored) = match &loaded {
            Ok(text) => (None, text.clone()),
            Err(_) => (Some(SaveGuard::ReadFailed), None),
        };
        (Self { storage, key, guard, stored }, loaded)
    }

    /// Record that the game could not use the text it read; ordinary writes leave it in place from now on.
    pub fn reject(&mut self) {
        if self.guard.is_none() {
            self.guard = Some(SaveGuard::Rejected);
        }
    }

    /// Why ordinary writes are refused, or `None` while they are allowed.
    pub fn guard(&self) -> Option<SaveGuard> {
        self.guard
    }

    /// Store `text` unless the slot is guarded or already holds it.
    pub fn write(&mut self, text: &str) -> Result<SaveWrite, StorageError> {
        if let Some(guard) = self.guard {
            return Ok(SaveWrite::Guarded(guard));
        }
        if self.stored.as_deref() == Some(text) {
            return Ok(SaveWrite::Unchanged);
        }
        self.storage.save(&self.key, text)?;
        self.stored = Some(text.to_owned());
        Ok(SaveWrite::Written)
    }

    /// Store `text` even over a guarded slot, because the player explicitly chose to replace what it held. A
    /// successful write lifts the guard; a failed one leaves it.
    pub fn replace(&mut self, text: &str) -> Result<(), StorageError> {
        self.storage.save(&self.key, text)?;
        self.guard = None;
        self.stored = Some(text.to_owned());
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("joy-save-slot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn unreadable_saves_are_never_overwritten_until_replaced() {
        let folder = folder("unreadable");
        // Bytes that are not text make the read fail even though a save exists.
        std::fs::write(folder.join("progress"), [0xff, 0xfe, 0x25]).unwrap();
        let (mut slot, loaded) = SaveSlot::open(Storage::in_directory(&folder), "progress");
        assert!(matches!(loaded, Err(StorageError::Read { .. })));
        assert_eq!(slot.guard(), Some(SaveGuard::ReadFailed));
        assert_eq!(slot.write("{\"level\":1}").unwrap(), SaveWrite::Guarded(SaveGuard::ReadFailed));
        assert_eq!(std::fs::read(folder.join("progress")).unwrap(), [0xff, 0xfe, 0x25]);

        slot.replace("{\"level\":1}").unwrap();
        assert_eq!(slot.guard(), None);
        assert_eq!(slot.write("{\"level\":2}").unwrap(), SaveWrite::Written);
        assert_eq!(std::fs::read_to_string(folder.join("progress")).unwrap(), "{\"level\":2}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn rejected_saves_stay_in_place_and_empty_slots_save_normally() {
        let folder = folder("rejected");
        let storage = Storage::in_directory(&folder);
        storage.save("progress", "{\"version\":99}").unwrap();
        let (mut slot, loaded) = SaveSlot::open(storage.clone(), "progress");
        assert_eq!(loaded.unwrap().as_deref(), Some("{\"version\":99}"));
        slot.reject();
        assert_eq!(slot.write("{\"version\":1}").unwrap(), SaveWrite::Guarded(SaveGuard::Rejected));
        assert_eq!(storage.load("progress").unwrap().as_deref(), Some("{\"version\":99}"));

        let (mut empty, loaded) = SaveSlot::open(storage.clone(), "settings");
        assert_eq!(loaded.unwrap(), None);
        assert_eq!(empty.write("a").unwrap(), SaveWrite::Written);
        assert_eq!(empty.write("a").unwrap(), SaveWrite::Unchanged);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
