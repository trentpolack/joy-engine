// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/progress_save.rs`: saved progress is never lost to an autosave, whether storage could not read it
//! or the game could not use it.

use super::*;
use crate::storage::MAXIMUM_STORAGE_BYTES;

const KEY: &str = "progress";

/// A game's progress: one level number saved as `level=<n>`.
fn restore(text: &str) -> Option<u32> {
    text.strip_prefix("level=")?.trim().parse().ok()
}

fn serialize(level: u32) -> String {
    format!("level={level}")
}

fn folder(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("joy-progress-save-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn load(folder: &std::path::Path) -> (ProgressSave, u32) {
    ProgressSave::load(Some(Storage::in_directory(folder)), KEY, restore, || 1)
}

#[test]
fn unreadable_saves_survive_autosaves() {
    let folder = folder("unreadable");
    // A valid save the store refuses to read (past its size limit), while small writes would still succeed.
    let unreadable = format!("{}{}", serialize(25), " ".repeat(MAXIMUM_STORAGE_BYTES));
    std::fs::write(folder.join(KEY), &unreadable).unwrap();

    let (mut save, level) = load(&folder);
    assert_eq!(level, 1, "the session starts fresh");
    assert_eq!(save.status(), SaveStatus::StorageUnavailable);
    save.save(&serialize(level));
    assert_eq!(std::fs::read_to_string(folder.join(KEY)).unwrap(), unreadable, "the autosave left the save alone");

    // Once the save can be read again, the progress returns.
    std::fs::write(folder.join(KEY), serialize(25)).unwrap();
    assert_eq!(load(&folder).1, 25);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn damaged_saves_are_preserved_and_readable_saves_keep_saving() {
    let folder = folder("damaged");
    std::fs::write(folder.join(KEY), "level=").unwrap();
    let (mut save, level) = load(&folder);
    assert_eq!(save.status(), SaveStatus::Unreadable);
    save.save(&serialize(level));
    assert_eq!(std::fs::read_to_string(folder.join(KEY)).unwrap(), "level=", "the damaged save is preserved");

    std::fs::remove_file(folder.join(KEY)).unwrap();
    let (mut save, _) = load(&folder);
    assert!(!save.status().is_problem());
    save.save(&serialize(2));
    assert_eq!(load(&folder).1, 2);
    let _ = std::fs::remove_dir_all(&folder);
}
