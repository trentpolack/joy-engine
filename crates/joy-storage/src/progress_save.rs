// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A game's saved progress in one call: open the slot, restore what it holds (or start fresh), keep a save that could
//! not be read or used from ever being overwritten, and report what happened for the game's status line.
//!
//! This wraps [`SaveSlot`] with the policy every game repeated: storage failures and unreadable saves start a fresh
//! session that persists nothing over the old save, write failures keep play going, and each outcome is a
//! [`SaveStatus`] the game words for its players.

use crate::save_slot::SaveSlot;
use crate::storage::Storage;

/// What the latest load or save found, for a status line. Games own the wording.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SaveStatus {
    /// Progress loads and saves normally (or nothing was saved yet).
    #[default]
    Ready,
    /// Storage could not be read: this session will not persist, and any existing save is left untouched.
    StorageUnavailable,
    /// The saved text could not be restored: play starts fresh and the old save is preserved.
    Unreadable,
    /// A save failed: progress lasts for this session only.
    WriteFailed,
}

impl SaveStatus {
    /// Whether the status is worth showing the player.
    pub fn is_problem(self) -> bool {
        self != Self::Ready
    }
}

/// A game's saved progress: its guarded slot (or none, for sessions that keep progress in memory) and its status.
#[derive(Clone, Debug, Default)]
pub struct ProgressSave {
    // `None` keeps progress in memory only (tests and captures).
    slot: Option<SaveSlot>,
    status: SaveStatus,
}

impl ProgressSave {
    /// Open `key` in `storage` and restore the saved value with `restore` (which returns `None` for text it cannot
    /// use), starting from `fresh()` when nothing usable is saved. `None` storage keeps progress in memory only.
    ///
    /// A read failure or unusable text guards the slot, so later [`save`](Self::save)s never overwrite what is there,
    /// and sets the status the game reports.
    pub fn load<T>(
        storage: Option<Storage>,
        key: &str,
        restore: impl FnOnce(&str) -> Option<T>,
        fresh: impl FnOnce() -> T,
    ) -> (Self, T) {
        let Some(storage) = storage else {
            return (Self::unsaved(), fresh());
        };
        let (mut slot, loaded) = SaveSlot::open(storage, key);
        let mut status = SaveStatus::Ready;
        let text = match loaded {
            Ok(text) => text,
            Err(error) => {
                log::warn!("{error}");
                status = SaveStatus::StorageUnavailable;
                None
            }
        };
        let restored = text.as_deref().and_then(restore);
        if text.is_some() && restored.is_none() {
            slot.reject();
            status = SaveStatus::Unreadable;
        }
        let value = restored.unwrap_or_else(fresh);
        (
            Self {
                slot: Some(slot),
                status,
            },
            value,
        )
    }

    /// Progress kept in memory only.
    pub fn unsaved() -> Self {
        Self::default()
    }

    /// Persist `text` (the game's serialized progress). Failures never interrupt play: they are logged and reported
    /// as [`SaveStatus::WriteFailed`], and a guarded slot is left alone. Unchanged text is not written again.
    pub fn save(&mut self, text: &str) {
        let Some(slot) = &mut self.slot else {
            return;
        };
        if let Err(error) = slot.write(text) {
            log::warn!("{error}");
            self.status = SaveStatus::WriteFailed;
        }
    }

    /// What the latest load or save found.
    pub fn status(&self) -> SaveStatus {
        self.status
    }

    /// The guarded slot, for explicit replacement (for example a new campaign) through [`SaveSlot::replace`].
    pub fn slot_mut(&mut self) -> Option<&mut SaveSlot> {
        self.slot.as_mut()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "progress_save.test.rs"]
mod tests;
