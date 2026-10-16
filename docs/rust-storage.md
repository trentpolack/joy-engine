# Rust Storage
`runtime::Storage` (platform layer, `src/runtime/storage.rs` with browser glue in `src/browser/storage.rs`) keeps small text values such as game saves across sessions. It is the Rust counterpart of TypeScript games calling `localStorage` directly; LUNA's character save is its first consumer.

## API
```rust
use joy_engine::runtime::{Storage, StorageError};

let storage = Storage::new("luna");                       // native folder name; browsers use the page origin
let saved: Option<String> = storage.load("joy.luna.character.v1")?;
storage.save("joy.luna.character.v1", &serialized)?;
storage.remove("joy.luna.character.v1")?;
```

- Keys are 1 to 128 ASCII letters, digits, periods, hyphens, or underscores and may not start with a period, so the same key is a valid `localStorage` key and a portable file name. Values are text of at most 1 MiB (`MAXIMUM_STORAGE_BYTES`); callers serialize and validate them.
- Browsers read and write the page origin's `localStorage`. Native builds keep one file per key in `<data>/joy-games/<application>/`, where `<data>` is `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, and `$XDG_DATA_HOME` or `~/.local/share` elsewhere. `Storage::in_directory` selects an explicit folder for tools and tests.
- Native saves write a temporary file and rename it over the value, so an interrupted save keeps the previous value. Missing values load as `None`; removing a missing value succeeds.
- Every failure is a `StorageError` (`Unavailable`, `InvalidKey`, `TooLarge`, `Read`, or `Write`) rather than a panic, so a game can keep playing and tell the player that progress will not persist, as the TypeScript games do when `localStorage` throws.

## Saves: `SaveSlot`
A failed read does not mean nothing is saved. The value may exist and be unreadable for now (a permission problem, a file that is not text), and native storage can often still be written, so a game that treats the failure as an empty slot overwrites the player's progress on its next autosave. Keep progress in a `runtime::SaveSlot` instead:

```rust
use joy_engine::runtime::{SaveSlot, SaveWrite, Storage};

let (mut slot, loaded) = SaveSlot::open(Storage::new("luna"), "joy.luna.character.v1");
let character = match loaded {
    Ok(Some(text)) => restore(&text).unwrap_or_else(|| { slot.reject(); fresh() }),  // damaged: keep it
    Ok(None) => fresh(),
    Err(error) => fresh(),                         // the slot is now guarded: nothing will overwrite it
};
slot.write(&serialize(&character))?;               // Written, Unchanged, or Guarded(reason)
slot.replace(&serialize(&new_campaign))?;          // the player explicitly started over: lifts the guard
```

After a failed read (`SaveGuard::ReadFailed`) or a `reject` (`SaveGuard::Rejected`), ordinary writes return `SaveWrite::Guarded` and leave the stored value untouched until the game calls `replace` in response to an explicit player choice. Unchanged text is not rewritten. LUNA and FRAMEBREAK keep their progress this way; VECTORS TO VICTORY applies the same rule through its own store, and its New Campaign command is the explicit replacement.

## Validation
Unit tests in `src/runtime/storage.rs` cover round trips, replacement without leftover temporary files, removal, key validation, size limits, and unavailable stores; `src/runtime/save_slot.rs` covers guarded slots after unreadable and rejected saves, explicit replacement, and unchanged writes. Browser behavior is exercised by the LUNA web build's save and reload.
