# PARTICLE LAB
This workspace contains the playable project, its project-owned content, and its build configuration.

See the [project documentation](https://github.com/trentpolack/JoyGames/blob/develop/docs/projects/particle-lab/README.md) for the application overview, controls, architecture, development workflow, and validation guidance.

### Live preview application
Live applies valid definition edits after the existing debounce. Turning it off cancels queued automatic work and prevents an automatic preparation already in flight from replacing the preview. Explicit Apply remains available with Live off, and enabling Live applies pending edits. Invalid drafts stay separate from the current valid definition and accepted preview; repair them before Apply or export. Run `npm run test:live-browser --workspace=@joy-games/particle-lab` for the queued/in-flight/manual application regression, using `CAPTURE_EXECUTABLE` for a locally installed Chrome.

### Parameter Editing
Bounded scalars pair a slider with an exact numeric field. XYZ and RGBA components stay together in compact groups; Bounds remains available beneath each parameter. Color swatches display linear RGB in sRGB, and the native picker converts an explicit RGB selection back to linear within existing bounds while preserving alpha. Exact fields preserve their full authored precision. Property Undo/Redo includes picker choices, and Shift/Alt + arrow keys retain fine/coarse editing. A rejected numeric draft must be repaired before a picker choice or export can replace accepted values.

### Rust
PARTICLE LAB also runs on the Rust Joy Engine from the `particle-lab` package in this folder (`Cargo.toml`, modules in `src/*.rs` beside the TypeScript they replace), natively and in browsers, and Joy Editor hosts its `ParticleLabTool` as a tab through `AuthoringTool`. The preview draws only through the engine renderer (particles, editor grid, light receiver, and display processing), and the interface is built from the engine's [authoring kit](../../../docs/rust-authoring.md).

```bash
cargo run -p particle-lab                                   # native window
cargo test -p particle-lab                                  # document, examples, textures, session, and headless interface tests
node scripts/rust/build-web.mjs --package particle-lab      # browser page (add --release for a small build)
```

Module map: `document.rs` (portable and recovery reads, export, property snapshots, and document edits), `examples.rs`, `storage.rs`, and `lab.rs` (the session: Live application with debounce, playback, seeking, undo, recovery, and monitoring) hold no interface code; `editor.rs`, `emitter_inspector.rs`, `parameter_controls.rs`, and `interface.rs` are the egui interface; `viewport.rs` draws the preview; `tool.rs` implements `Game` and `AuthoringTool`.

Differences from the TypeScript lab:
- Applying is synchronous: compiling, starting the effect, and decoding its textures happen in one frame, so there is no in-flight preparation for Live to cancel; turning Live off cancels the queued apply.
- Opened documents are normalized by the engine compiler (defaults written out, bursts sorted by time), so a saved file can differ textually from the opened one while meaning the same effect.
- The recovery key is `joy-games.particle-lab.v1` (storage keys are portable file names), and natively the draft lives in the per-user data folder; it is written at most twice per second while editing.
- The preview renders into its panel region of the window (`rendering::RegionCamera`); display processing such as the vignette is centered on the window rather than the preview.
- Script editors highlight the effect's parameter names, mark script errors in place with line and column, and complete calls with the cursor between the parentheses.
- Number fields also scrub by dragging, and Escape in a field restores the accepted value.
- The color picker is the kit's HSV picker with sRGB hex entry instead of the browser's native picker.
