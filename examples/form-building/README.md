# FORM Building Shell
A Rust example of editable procedural content and baked consumption. `assets/building.form` produces a reusable exterior in meters, Y-up, centered X/Z, with ground at Y=0. Windows and doors are decorative; no playable interior, collision, or navigation is implied.

```bash
npm run dev --workspace @joy-engine/example-form-building
npm run build --workspace @joy-engine/example-form-building
npm run test --workspace @joy-engine/example-form-building
npm run capture --workspace @joy-engine/example-form-building -- --built
```

The native command runs the Cargo example. Browser output lives in this workspace's `dist/`, published by JoyGames at `/examples/form-building/`. Use `?mode=baked` for the baked cottage; natively pass `--mode=baked` to `cargo run --example form-building`. Browser `backend=webgpu` or `backend=webgl` selects rendering.

## Editable and Baked Content
`CompiledScript` parses once, then evaluates complete parameter snapshots. `form_mesh_asset` draws accepted output with the engine PBR pass, including authored material factors. Invalid evaluation retains the previous building. Presets include cottage, warehouse, and apartments; counts are explicitly floored, while metadata bounds do not quantize arbitrary inputs.

Open the supplied `.forminst` instances (cottage, warehouse, and apartments, each referring to `building.form`) in FORM EDITOR to edit parameters, materials, and mesh resources. Save editable recipe writes a packed `building.forminst` with the script and the current overrides; Bake mesh JSON writes evaluated geometry. Browser actions download; native actions write their named output files. GLB exports are baked interchange and do not preserve procedural source. Baked mode loads the supplied mesh without evaluating FORM.

## Validation and Limits
`tests/integration/example_form_building.rs` covers presets, failure isolation, baked round trips, tool documents through `FormAssetLibrary`, and headless renders. Browser captures provide startup/image evidence. CPU frame encoding is not GPU execution time.

The example is not a DCC bridge, node graph, automatic collision/LOD generator, or runtime fracture system. FORM remains the source of geometry, and the host remains its consumer.
