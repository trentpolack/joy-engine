# JOYFX EDITOR
JOYFX EDITOR authors `.joyfx` effects with per-emitter scripts, parameters, materials, and a live Rust preview. It runs natively and in browsers and implements `AuthoringTool` for Joy Editor tabs. The engine renderer draws particles, the reference grid, optional light receiver, and display processing.

```bash
npm run dev --workspace @joy-games/joyfx-editor
npm run build --workspace @joy-games/joyfx-editor
npm run test --workspace @joy-games/joyfx-editor
```

`dev` runs the native `joyfx-editor` Cargo application; `build` writes browser output to `dist/`. The shared [authoring kit](../../docs/rust-authoring.md) supplies egui panels, numeric controls, script editing, history, file dialogs, and viewport gestures.

## Editing
Live applies valid edits after a debounce; turning it off cancels queued work, while explicit Apply remains available. Compilation, effect startup, and texture decoding happen synchronously. Rejected drafts never replace the accepted effect. Scripts show language highlighting, completion, hover help, and source-located diagnostics.

Numeric fields preserve authored precision and support scrubbing, exact entry, defaults/reset, and property undo. Color controls use linear RGBA values with an sRGB picker; opening a picker never changes data. Script undo and property undo remain separate. Preview camera, lighting, and replay windows do not alter exported assets.

Open/save use system dialogs natively and browser file/download behavior on the web. Accepted documents are normalized by the engine, so formatting/defaults can change without changing effect meaning. Recovery uses `joy-games.particle-lab.v1` through engine storage and writes at most twice per second while editing. Recovery is not a source-file save.

## Preview Performance and Display Range
The preview reuses particle geometry while the particles, camera basis, and reference grid are unchanged, including paused frames and frames between simulation steps. Direct particle edits, stepping, seeking, applying an effect, and changing the camera or grid invalidate that geometry. Detailed geometry timings describe work in the last render, so cached frames retain triangle counts but report zero per-emitter generation and sorting time. Submit CPU measures command preparation, not GPU execution.

The range monitor reports scene precision separately from the renderer's configured display signal. `HDR 3.5x Output` means an extended-range signal with headroom of 3.5 times SDR white. `HDR 1.0x Output` still uses the HDR signal, but the display currently reports no highlight headroom. The native shell enables HDR on supported displays; this readout no longer assumes SDR output.

The engine workspace and JoyGames optimize `joy-renderer` at level 2 in development builds. To measure the actual Ember Bloom preset on the CPU without GPU or frame-pacing costs, run from the engine root:

```bash
cargo test -p joyfx-editor ember_bloom -- --ignored --nocapture
```

The benchmarks use seed 777 at 0.683 seconds and print median and p95 geometry times, including the unchanged-preview cache. These are CPU measurements, not predictions of display FPS.

## Modules and Tests
Document, example, storage, and session modules under `src/` remain UI-free; editor/inspector/interface modules present them, and `tool.rs` implements runtime and hosting contracts. Rust tests cover documents, playback, textures, history, and real headless UI interactions. Browser captures smoke-test canvas startup and images. See the [effect format](../../docs/particles.md).
