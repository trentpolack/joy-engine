# PARTICLE LAB
PARTICLE LAB authors `.joyfx` effects with per-emitter scripts, parameters, materials, and a live Rust preview. It runs natively and in browsers and implements `AuthoringTool` for Joy Editor tabs. The engine renderer draws particles, the reference grid, optional light receiver, and display processing.

```bash
npm run dev --workspace @joy-games/particle-lab
npm run build --workspace @joy-games/particle-lab
npm run test --workspace @joy-games/particle-lab
```

`dev` runs the native `particle-lab` Cargo application; `build` writes browser output to `dist/`. The shared [authoring kit](../../../docs/rust-authoring.md) supplies egui panels, numeric controls, script editing, history, file dialogs, and viewport gestures.

## Editing
Live applies valid edits after a debounce; turning it off cancels queued work, while explicit Apply remains available. Compilation, effect startup, and texture decoding happen synchronously. Rejected drafts never replace the accepted effect. Scripts show language highlighting, completion, hover help, and source-located diagnostics.

Numeric fields preserve authored precision and support scrubbing, exact entry, defaults/reset, and property undo. Color controls use linear RGBA values with an sRGB picker; opening a picker never changes data. Script undo and property undo remain separate. Preview camera, lighting, and replay windows do not alter exported assets.

Open/save use system dialogs natively and browser file/download behavior on the web. Accepted documents are normalized by the engine, so formatting/defaults can change without changing effect meaning. Recovery uses `joy-games.particle-lab.v1` through engine storage and writes at most twice per second while editing. Recovery is not a source-file save.

## Modules and Tests
Document, example, storage, and session modules under `src/` remain UI-free; editor/inspector/interface modules present them, and `tool.rs` implements runtime and hosting contracts. Rust tests cover documents, playback, textures, history, and real headless UI interactions. Browser captures smoke-test canvas startup and images. See the [effect format](../../../docs/particles.md).
