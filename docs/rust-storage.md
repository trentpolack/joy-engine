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

## Validation
Unit tests in `src/runtime/storage.rs` cover round trips, replacement without leftover temporary files, removal, key validation, size limits, and unavailable stores. Browser behavior is exercised by the LUNA web build's save and reload.
