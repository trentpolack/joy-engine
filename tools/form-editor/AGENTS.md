# FORM EDITOR Agent Guidance
Read the maintained [FORM product direction](https://github.com/trentpolack/JoyGames/blob/develop/docs/projects/form-editor/product-direction.md) before changing product behavior or UX. In a JoyGames checkout, its local source is `docs/projects/form-editor/product-direction.md` from the JoyGames root; do not duplicate the spec here or in another engine checkout.

- Keep scripting first-class and reuse the same tool implementation and formats for standalone and hosted use.
- Preserve PBR/shadow behavior while polishing authoring controls; preview-only lighting must remain separate from authored/exported data.
- Distinguish browser recovery, document saving, the last accepted build, and baked export. Never imply baked geometry retains the recipe or that browser recovery writes source to disk.
- Runtime destruction, standalone SDK/DCC workflows, and optional visual composition remain goals to investigate, not existing capability claims or required architecture.
- Respect the direction document's sequencing and approved scope. Trent cleared the rendering hold for a bounded UX pass on October 11, 2026; broader work still requires its own scope decision.

## Rust FORM EDITOR
- The Rust tool (`Cargo.toml`, `src/**/*.rs`) depends only on `joy-engine`. Missing capabilities (image formats, file dialogs, widgets) belong in the engine behind an engine API, and shared egui widgets in the authoring kit (`joy_engine::development`), not in this package.
- Keep UI modules (egui) separate from document and model modules so a later UI technology change touches only UI code.
- The preview must stay on the engine renderer (`form_mesh_asset`, the PBR pass, image lighting, shadows, `EditorGrid`); do not add tool shaders or batches.
- Run `cargo +nightly fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy --target wasm32-unknown-unknown -p form-editor --lib -- -D warnings`, `cargo test -p form-editor`, and `node ../../scripts/check-boundaries.mjs` before committing Rust changes.
