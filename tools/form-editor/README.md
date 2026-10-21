# FORM EDITOR
FORM EDITOR is a Rust procedural geometry workbench, available natively, in browsers, and as a Joy Editor tab. Script, parameter/material inspectors, and a 3D preview share the engine's [authoring kit](../../docs/rust-authoring.md). Product goals and open decisions remain in the [maintained direction](https://github.com/trentpolack/JoyGames/blob/develop/docs/projects/form-editor/product-direction.md).

```bash
npm run dev --workspace @joy-games/form-editor
npm run build --workspace @joy-games/form-editor
npm run test --workspace @joy-games/form-editor
npm run test:parity --workspace @joy-games/form-editor
```

`dev` runs the native `form-editor` Cargo application; `build` writes its browser release to `dist/`. The parity harness compares the Rust evaluator to the frozen FORM v2 reference fixtures; every case must pass (see [FORM validation](../../docs/rust-form.md#validation)).

## Editing and Files
FORM is script-first. Live compilation debounces changes; explicit Run and source-located diagnostics support iteration. Failed or expired builds keep the last accepted geometry, which exports use. Native compilation runs in a background thread; browser evaluation runs inline with bounded work. The engine owns compiler validation and budgets.

Examples are authored `.form` scripts and `.forminst` FORM instances discovered under the engine's `assets/examples` and `assets/form`; browser builds bundle them. Opening an example creates a working copy. Parameter defaults/reset, materials, up to 16 named variants, recipes, output inspection, resizable panels, View visibility, and Maximize remain authoring features.

Save defaults to raw `.form` source. Choose `.forminst` to preserve overrides, document materials/maps, imported meshes, and variants; an instance opened from a project keeps referring to its script, scripts read their `use` modules from the project or library folder, and version 1 `.formlab` documents open as packed instances. Saving source never bakes inspector overrides into expressions. GLB, OBJ, PLY, JSON, and MaterialX exports are baked interchange; they do not preserve the recipe. Native dialogs write chosen files; browsers download. Recovery through engine storage is separate from a source save.

## Rendering and Ownership
FORM returns geometry without owning a renderer. `form_mesh_asset` creates the preview asset, and the shared PBR pass draws its materials, PNG/JPEG/WebP maps, studio image lighting, direct light, and shadows on WebGPU and WebGL 2. `EditorGrid` is an editor-only aid. Lighting, camera, and inspection state never modify the authored document or export.

Document/model modules remain separate from egui UI modules (the two groups in `src/lib.rs`). Rust tests cover document/export contracts, examples, compiler lifecycle, and headless interface behavior. Browser capture is a startup/image smoke check. See the [FORM language reference](../../docs/rust-form.md) and the [FORM EDITOR application guide](https://github.com/trentpolack/JoyGames/blob/develop/docs/projects/form-editor/README.md).
