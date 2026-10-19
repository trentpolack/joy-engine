# Rust Terrain
`rendering::terrain` implements resident continuous-LOD terrain. Version-1 assets draw as lit, shadowed `RenderScene` instances or through `TerrainRenderer`. CPU selection and sampling retain recorded reference behavior. See [the terrain example](../examples/terrain/README.md) for assets, conversion, units, and orientation.

## Module Map
Paths are relative to `crates/`.

| Rust | Contents |
| --- | --- |
| `joy-renderer/src/terrain/terrain_asset.rs` | `TerrainAsset`, `TerrainManifest`, `TerrainPatch`, `decode_terrain`, `parse_terrain`, `load_terrain`, `sample_terrain`, `sample_height_triangle`, `TerrainError`. |
| `joy-renderer/src/terrain/terrain_grid.rs` | `create_terrain_grid`, `TerrainGrid`, `TerrainIndexRange`. |
| `joy-renderer/src/terrain/terrain_selection.rs` | `select_terrain`, `reconcile_terrain_edges`, `sample_morphed_height`, `validate_terrain_view`, `TerrainView`, `TerrainSelection`. |
| `joy-renderer/src/terrain/terrain_timing.rs` | `TerrainTiming`, `TerrainTimingStatus`. |
| `joy-renderer/src/terrain/terrain_renderer.rs` | `TerrainRenderer`, `TerrainFrame`, `TerrainStats`, `TerrainDiagnostic`, `TerrainTriangleCount`, and the resident geometry every terrain pass draws. |
| `joy-renderer/src/terrain/terrain_scene_pass.rs` | The renderer's per-instance adapters (crate-internal). |
| `joy-renderer/src/scene/terrain_instance.rs` | `TerrainInstance`, `DEFAULT_TERRAIN_TOLERANCE`; `RenderScene` gained `TerrainInstanceId` and terrain storage. |
| `joy-renderer/src/shaders/terrain-*.wgsl` | `terrain-geometry.wgsl` (shared morph), `terrain.wgsl` (standalone), `terrain-select.wgsl` (compute), `terrain-scene.wgsl`, `terrain-caster.wgsl`, `terrain-surface.wgsl`, `terrain-motion.wgsl`. wgpu translates these programs for each backend. |

## Scene Terrain
Add a terrain instance beside meshes; the renderer creates its GPU resources on first draw and releases them when the instance is removed or hidden:

```rust
use joy_engine::rendering::terrain::{parse_terrain, sample_terrain};
use joy_engine::rendering::TerrainInstance;

let asset = parse_terrain(&manifest_json, &height_bytes)?;     // or load_terrain(path) natively
let terrain = scene.insert_terrain(TerrainInstance::new(asset.clone()));
if let Some(instance) = scene.terrain_mut(terrain) {
    instance.target_radius = 90.0;                               // pin finest detail around the camera focus
}
let ground = sample_terrain(&asset, x, z);                      // gameplay height: finest surface, never camera LOD
target.draw(&scene, &camera, &inputs)?;
```

Each frame the renderer selects LODs once per instance in the terrain's local space (eye and focus from the stable camera, CSS projection scale from the logical viewport height), then draws the same indexed, morphed surface into every shadow view (directional cascades and point-light faces), the lit scene pass after opaque meshes, and, when scene effects run, the normal/roughness and motion attachments. Terrain is lit by `standardLighting` like Lit meshes, with slope and altitude coloring and the `TerrainDiagnostic` modes. Its world bounds join the shadow casters' bounds, and it counts toward `RendererDiagnostics` visible, culled, draw-call, and triangle totals. Invalid settings (non-positive tolerance, negative or non-finite target radius, a non-finite transform) fail the frame with `RenderError::Terrain` before any pass is recorded; a collapsed transform pins the finest geometry instead of inverting.

## Standalone Pass
`TerrainRenderer` provides a standalone pass: derivative-normal diffuse coloring with display gamma, drawn straight into a presentation target with its own depth. Create it in `Game::start` from `context.renderer.gpu()` and `context.renderer.output_format()`, then call `RenderTarget::draw_terrain` once per frame with a `TerrainFrame` (view-projection, `TerrainView`, diagnostic, clear color, and a millisecond clock that paces timing samples). Outside the shell, `prepare` (before the pass) and `draw` (into a caller pass with a `Depth24Plus` attachment), or `encode`, do the same work. `stats()` reports patches, triangles (an upper bound with GPU selection), CPU prepare and record times, the latest GPU timestamp sample, resident CPU and GPU bytes, and per-frame upload bytes.

## Backends
WebGPU, Vulkan, Metal, and DirectX 12 select LODs with two ordered compute dispatches that write patch records and indexed indirect draws; nothing is read back. WebGL 2 (and native OpenGL) runs the same policy on the CPU and uploads the records. Both paths bind each patch record at a dynamic uniform offset, so indirect draws keep `firstInstance` at zero. Heights are an `R32Float` texture fetched by texel in the vertex stage, which WebGL 2 supports; terrain draws, casts, and receives shadows there too. Terrain pipelines use bind group 3 and are compiled when the first terrain appears. `GpuContext` now enables `TIMESTAMP_QUERY` when the adapter offers it, so `TerrainTiming` samples the standalone pass at most twice per second without blocking; elsewhere it reports `unsupported`.

## Renderer Hook
The renderer changes are additive and isolated: one `SceneTerrain` field built with the caster bias, one `prepare` call after the camera is built (its counts and caster bounds seed the frame's totals), and one draw call each in the shadow-view loop, after `draw_opaque`, in the surface attachment pass, and in the motion attachment pass. `RenderTarget::draw_terrain` is the shell hook for the standalone pass, and the browser egui translator now hides the cursor when egui requests `CursorIcon::None`.

## Example
`examples/terrain/main.rs` runs Dragon Pit: `cargo run --example terrain`, or `node scripts/rust/build-web.mjs --example terrain` for browsers (`?backend=webgl` or `?backend=webgpu`). It provides camera controls (left-drag orbit, scroll zoom, right-button flight with WASD and Q/E, modifier-scroll pan, G and O presets, camera tour), the HUD statistics, and the CONFIG settings (screen error, LOD colors and grid, stable detail region) in the engine's development dock. CONFIG also switches the draw path between the standalone pass and the scene renderer, which shows the integration with lighting, cascaded shadows, and the sky. The heightfield and its MIT source notice are embedded in the binary; CONFIG shows the notice.

## Tests
`crates/joy-renderer/tests/integration/terrain.rs` compares camera selections, collision samples, descriptors, and morphing against `crates/joy-renderer/tests/fixtures/terrain-reference.json`. GPU selection tests compare read-back patch records and indirect commands to CPU selection. `crates/joy-renderer/tests/integration/rendering_terrain.rs` covers standalone diagnostics, lit scene terrain with shadows, motion/surface attachments, native OpenGL when available, and invalid settings.

## Ownership and Loading
Terrain resources release with their Rust owner. Assets are cheaply cloned shared handles. Native `load_terrain` reads a manifest and height file; browsers embed or fetch both and call `parse_terrain`. Instance settings are validated during frame preparation. The shell owns backing resolution; asset sampling remains independent of it.
