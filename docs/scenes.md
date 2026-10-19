# Mesh Assets and Rendering Scenes
`RenderScene` owns retained presentation records. Shared `MeshAsset` handles contain geometry; `MeshInstance`s carry transforms, tint, visibility, and material/shadow policy. Games own the simulation these records represent. The renderer borrows the scene and never updates gameplay.

Use `compile_form_mesh`, `form_mesh_asset`, or `FormAssetLibrary` for procedural assets and `MeshAsset::from_pbr_mesh` for textured material data. The [form-viewer example](../examples/form-viewer/main.rs) demonstrates the integration. A mesh uploads on first use, and its GPU copy is released when no live instance or asset handle needs it.

## Transforms and Collections
Positions use Y-up world coordinates. Rotations are XYZ Euler radians applied X, then Y, then Z; scale precedes rotation and translation. `RenderScene::insert_node` groups mesh and terrain instances through parent transforms and visibility. Presentation hierarchy does not imply a gameplay entity hierarchy.

`MeshCollection` synchronizes keyed simulation records into stable instances. Keep keys stable across removal/reordering, and prune absent records through the collection's update boundary. Removing presentation never destroys a simulation entity.

## Frame Inputs
The renderer uses one camera and frame state for culling, shadows, lighting, scene attachments, and display processing. Lit, PBR, and Unlit materials remain separate. Transparent geometry uses depth testing without depth writes and sorts back to front. `FrameInputs` can also borrow transient colored triangles, ordered textured draws, environment state, lights, and image lighting.

`LevelScene` translates authored render and light records into presentation, and `PostProcessVolumes` resolves `postProcess` volumes at the view point (see [postprocessing](postprocessing.md)). `AssetScene` adds FORM assets and colored/textured JOYFX effects; its staged `prepare` and `commit` retain accepted presentation on failure. See [particles](rust-particles.md).

Use the rendering integration tests in `tests/integration/` and browser captures for the affected projects. Inspect shadows, transparency, scene effects, and high-density scaling on every supported backend affected by the change.
