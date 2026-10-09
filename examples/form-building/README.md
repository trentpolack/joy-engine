# FORM building shell
A reusable exterior asset authored entirely in FORM. It uses meters, Y-up, centered X/Z, and ground Y=0. Four walls, thin floor slabs, facade panels, entrance canopies, and flat/gable roofs form a compact building that a game can borrow as a static prop. It has no physics, navigation, or playable interior. Doors and windows are decorative panels over walls.

## Run locally
From `/Users/trent/development/JoyGames`:
```sh
npm run dev --workspace @joy-engine/example-form-building
npm run check --workspace @joy-engine/example-form-building
npm run test --workspace @joy-engine/example-form-building
npm run bake --workspace @joy-engine/example-form-building
npm run build --workspace @joy-engine/example-form-building
npm run capture --workspace @joy-engine/example-form-building
npm run capture --workspace @joy-engine/example-form-building -- --built
```
Open the URL printed by Vite. Use `?backend=webgl` or `?backend=webgpu` to select a backend. Add `&mode=baked` to load the supplied cottage mesh without parsing or evaluating FORM. Drag to orbit, scroll to zoom, and click **Frame view** after large dimension changes. The sidebar scrolls independently of the asset viewport.

## Editable procedural use
`assets/building.form` is the canonical source; `assets/presets.json` contains cottage, warehouse, and apartment configurations. The browser parses once with `compileFormScript(source)`, evaluates complete parameter snapshots, copies the successful result into an immutable asset, and replaces the scene's previous material groups. A failed evaluation leaves the previous visible asset intact. `src/asset.ts` and `src/presentation.ts` show game consumption through public engine exports; neither generates geometry.

The runtime panel exposes width/depth (4–30 m), total wall height (3–30 m), floors (1–8), window spacing (1.5–6 m), door spacing (3–12 m), roof shape (0 flat / 1 gable), roof rise (0.3–5 m), and seed (1–9999). Floors are explicitly floored; roof shape switches at 0.5. Metadata clamps finite inputs but does not quantize them. Total height is divided by floor count, so a short eight-floor shell is valid stress geometry, not plausible architecture. Keep normal floor heights around 2.5–3.5 m. The seed changes window tint rather than gameplay or structural dimensions.

Open `assets/cottage.formlab`, `warehouse.formlab`, or `apartments.formlab` in FORM LAB (`npm run dev:form-lab`, then **Open**). FORM LAB also exposes wall/roof RGBA colors, material resource overrides, and the replaceable window mesh. Saved resource overrides take precedence over the material defaults derived from tint. The runtime **Save editable recipe** button writes a complete `.formlab` source/input snapshot. `npm run bake` refreshes the three provided documents after source/preset edits.

## Baked consumption and export
`assets/cottage.mesh.json` contains evaluated geometry, material assignments, and semantic entrance metadata. The baked URL path constructs `new MeshAsset(data)` and never evaluates FORM; it cannot change procedural dimensions. The runtime **Bake mesh JSON** button saves the current evaluated asset. JSON preserves authoring material data but is an engine-local mesh format.

`assets/cottage.glb`, `warehouse.glb`, and `apartments.glb` were exported through FORM LAB. For a fresh interoperable export, open the corresponding `.formlab`, select **GLB · Geometry + materials**, and click **Export data**. Those GLBs retain material groups, metallic/roughness values, and vertex colors. They are baked; they carry no editable FORM source. GLB reload into a game is outside this example's exercised runtime path.

`SceneRenderer` does not interpret asset PBR materials. The example's presentation adapter groups existing triangles by material and uses supported instance tint/roughness, leaving the canonical asset untouched. Metallic, emissive, texture, and material alpha-mode behavior require a different runtime path; FORM LAB previews and GLB exports preserve the authored channels. This example does not add a new renderer subsystem.

The example owns its RAF, listeners, renderer, and scene. `dispose()` stops the loop, aborts listeners, destroys rendering resources, clears scene references, and releases the current asset. Mesh assets own CPU arrays only. Legacy FORM `instance()` reuses source meshes during authoring but realizes copies in final geometry; this example does not demonstrate GPU instancing or retained logical `instances()` outputs.

## Findings and evidence
See [the findings report](FINDINGS.md) for measured timings, the focused matrix, minimal diagnostic reproductions, browser evidence, and the existing repository-validation blocker. The example is an engine workspace, excluded from site discovery. No game or Vectors to Victory files changed.
