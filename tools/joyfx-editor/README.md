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

The emitter inspector edits each emitter's renderer and blend mode (`alpha`, `additive`, or `premultiplied`; see the [effect format](../../docs/particles.md#particle-materials)) beside its timing, texture, light, bursts, and child emission.

Numeric fields preserve authored precision and support scrubbing, exact entry, defaults/reset, and property undo. Color controls use linear RGBA values with an sRGB picker; opening a picker never changes data. Script undo and property undo remain separate. Preview camera, lighting, and replay windows do not alter exported assets.

Open/save use system dialogs natively and browser file/download behavior on the web. Accepted documents are normalized by the engine, so formatting/defaults can change without changing effect meaning. Recovery uses `joy-games.particle-lab.v1` through engine storage and writes at most twice per second while editing. Recovery is not a source-file save.

## Preview Performance and Display Range
The preview submits particles through the same `ParticleFrame` games use, rebuilt every frame into kept allocations: each particle is one GPU instance, so there is no per-emitter geometry to cache. Instance CPU measures building the frame's instances, grid, and lights; Submit CPU measures the renderer's culling, sorting, uploads, and command preparation, not GPU execution. Particles drawn counts instances after culling.

The range monitor reports scene precision separately from the renderer's configured display signal. `HDR 3.5x Output` means an extended-range signal with headroom of 3.5 times SDR white. `HDR 1.0x Output` still uses the HDR signal, but the display currently reports no highlight headroom. The native shell enables HDR on supported displays; this readout no longer assumes SDR output.

The engine workspace and JoyGames optimize `joy-renderer` at level 2 in development builds. To measure the actual Ember Bloom preset on the CPU without GPU or frame-pacing costs, run from the engine root:

```bash
cargo test -p joyfx-editor ember_bloom -- --ignored --nocapture
```

The benchmark uses seed 777 at 0.683 seconds and prints median and p95 times to build the preset's particle instances. These are CPU measurements, not predictions of display FPS.

## Modules and Tests
Document, example, storage, and session modules under `src/` remain UI-free; editor/inspector/interface modules present them, and `tool.rs` implements runtime and hosting contracts. Rust tests cover documents, playback, textures, history, and real headless UI interactions. Browser captures smoke-test canvas startup and images. See the [effect format](../../docs/particles.md).
