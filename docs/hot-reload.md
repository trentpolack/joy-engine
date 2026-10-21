# Asset Hot Reload
Games embed their authored text assets so browser and release builds need no file system, and native development builds read the same assets from the project folder so edits apply without recompiling or restarting.

## GameAssets
`joy_engine::game_assets!["assets/main.joylevel", "assets/form/tower.form"]` embeds each file (paths relative to the game's manifest directory, always under `assets/`) and, in native development builds, remembers the project folder. Games that already embed text can build the table themselves with `GameAssets::new(entries, joy_engine::source_root!())`.

- `read(path)` returns the file on disk while hot reloading (falling back to the embedded text when the file cannot be read) and the embedded text otherwise. Paths outside `assets/`, absolute paths, and parent segments are refused.
- `poll_changes(elapsed_seconds)` checks, at most every `ASSET_POLL_SECONDS`, the stamps of files read so far and returns the paths that changed. A path stays reported until the game reads it again, so a refused edit (for example FORM that no longer parses) is retried after the next save.
- Browser and release builds report `hot_reload() == false`, never touch files, and always return the embedded text.

## Rebuilding What Changed
Reload only what an edit touched, and keep the previous result when the new text is rejected:

- FORM scripts and instances in levels: prepare the presentation again (`AssetScene::prepare` keeps running effects whose text and settings are unchanged, and a failed preparation leaves the displayed scene intact). `FormAssetLibrary` recompiles an asset when its own text, an instance's referenced script, or any `use` module it read changes.
- Game-owned FORM programs: hand the new source to the owner's compile cache (FRAMEBREAK's part and city libraries use `replace_source`). A script's modules are its dependencies too; `CompiledScript::module_paths()` lists the files to watch.
- `.joyfx` effects: restart the effects that use the changed definition.
- `.joylevel`: rebuild the level session when the game can restart it safely.

Embed the referenced scripts and modules a FORM asset reads alongside it, since browser and release builds can only read embedded files.

Joy Editor's **Play** runs a game in its own window; saved assets reach it through this path, so authoring and playing stay live together.

## Limits
Code changes still rebuild the game; JoyGames' `Cargo.toml` keeps engine crates lightly optimized in development builds so engine edits rebuild quickly. Shaders compiled into the renderer are not hot reloaded yet. The browser development server rebuilds the WebAssembly page when assets change, because browser builds embed them.
