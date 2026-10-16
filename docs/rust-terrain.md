# Rust Terrain
The Rust `rendering::terrain` module ports the TypeScript resident continuous-LOD (CLOD) terrain (`src/rendering/terrain/`, `src/rendering/scene/terrain-instance.ts`, and `shaders/rendering/terrain/*.wgsl`). The same version 1 assets load in both engines, CPU selection and sampling match the TypeScript results bit for bit, and terrain draws through `RenderScene` terrain instances (lit, shadowed, and in scene-effect attachments) or through the standalone `TerrainRenderer`. Asset format, offline conversion, units, and orientation are described in [the terrain example guide](../examples/terrain/README.md).

## Module Map
| TypeScript | Rust | Contents |
| --- | --- | --- |
| `terrain/terrain-asset.ts` | `rendering/terrain/terrain_asset.rs` | `TerrainAsset`, `TerrainManifest`, `TerrainPatch`, `decode_terrain`, `parse_terrain`, `load_terrain`, `sample_terrain`, `sample_height_triangle`, `TerrainError`. |
| `terrain/terrain-grid.ts` | `rendering/terrain/terrain_grid.rs` | `create_terrain_grid`, `TerrainGrid`, `TerrainIndexRange`. |
| `terrain/terrain-selection.ts` | `rendering/terrain/terrain_selection.rs` | `select_terrain`, `reconcile_terrain_edges`, `sample_morphed_height`, `validate_terrain_view`, `TerrainView`, `TerrainSelection`. |
| `terrain/terrain-timing.ts` | `rendering/terrain/terrain_timing.rs` | `TerrainTiming`, `TerrainTimingStatus`. |
| `terrain/terrain-renderer.ts` | `rendering/terrain/terrain_renderer.rs` | `TerrainRenderer`, `TerrainFrame`, `TerrainStats`, `TerrainDiagnostic`, `TerrainTriangleCount`, and the resident geometry every terrain pass draws. |
| `terrain/terrain-scene-pass.ts` | `rendering/terrain/terrain_scene_pass.rs` | The renderer's per-instance adapters (crate-internal). |
| `scene/terrain-instance.ts` | `rendering/scene/terrain_instance.rs` | `TerrainInstance`, `DEFAULT_TERRAIN_TOLERANCE`; `RenderScene` gained `TerrainInstanceId` and terrain storage. |
| `shaders/rendering/terrain/*.wgsl` | `rendering/shaders/terrain-*.wgsl` | `terrain-geometry.wgsl` (shared morph), `terrain.wgsl` (standalone), `terrain-select.wgsl` (compute), `terrain-scene.wgsl`, `terrain-caster.wgsl`, `terrain-surface.wgsl`, `terrain-motion.wgsl`. The GLSL variants are unnecessary with wgpu. |

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
`TerrainRenderer` is the TypeScript example's pass: derivative-normal diffuse coloring with display gamma, drawn straight into a presentation target with its own depth. Create it in `Game::start` from `context.renderer.gpu()` and `context.renderer.output_format()`, then call `RenderTarget::draw_terrain` once per frame with a `TerrainFrame` (view-projection, `TerrainView`, diagnostic, clear color, and a millisecond clock that paces timing samples). Outside the shell, `prepare` (before the pass) and `draw` (into a caller pass with a `Depth24Plus` attachment), or `encode`, do the same work. `stats()` reports patches, triangles (an upper bound with GPU selection), CPU prepare and record times, the latest GPU timestamp sample, resident CPU and GPU bytes, and per-frame upload bytes.

## Backends
WebGPU, Vulkan, Metal, and DirectX 12 select LODs with two ordered compute dispatches that write patch records and indexed indirect draws; nothing is read back. WebGL 2 (and native OpenGL) runs the same policy on the CPU and uploads the records. Both paths bind each patch record at a dynamic uniform offset, so indirect draws keep `firstInstance` at zero. Heights are an `R32Float` texture fetched by texel in the vertex stage, which WebGL 2 supports; terrain draws, casts, and receives shadows there too. Terrain pipelines use bind group 3 and are compiled when the first terrain appears. `GpuContext` now enables `TIMESTAMP_QUERY` when the adapter offers it, so `TerrainTiming` samples the standalone pass at most twice per second without blocking; elsewhere it reports `unsupported`.

## Renderer Hook
The renderer changes are additive and isolated: one `SceneTerrain` field built with the caster bias, one `prepare` call after the camera is built (its counts and caster bounds seed the frame's totals), and one draw call each in the shadow-view loop, after `draw_opaque`, in the surface attachment pass, and in the motion attachment pass. `RenderTarget::draw_terrain` is the shell hook for the standalone pass, and the browser egui translator now hides the cursor when egui requests `CursorIcon::None`.

## Example
`examples/terrain/main.rs` ports the Dragon Pit example: `cargo run --example terrain`, or `node scripts/rust/build-web.mjs --example terrain` for browsers (`?backend=webgl` or `?backend=webgpu`). It keeps the TypeScript camera controls (left-drag orbit, scroll zoom, right-button flight with WASD and Q/E, modifier-scroll pan, G and O presets, camera tour), the HUD statistics, and the CONFIG settings (screen error, LOD colors and grid, stable detail region) in the engine's development dock. CONFIG also switches the draw path between the standalone pass and the scene renderer, which shows the integration with lighting, cascaded shadows, and the sky. The heightfield and its MIT source notice are embedded in the binary; CONFIG shows the notice.

## Tests
`test/rust/terrain.rs` ports `test/terrain.test.mjs` and compares Dragon Pit selection details for four camera views, collision samples, the near-ground descriptors, and morphed heights exactly with `test/fixtures/terrain-reference.json`, recorded from the TypeScript modules under Node 22. A unit test in `terrain_renderer.rs` reads back the compute path's patch records and indirect commands and compares them with CPU selection (the TypeScript browser parity check). `test/rust/rendering_terrain.rs` renders the standalone pass in every diagnostic mode, scene terrain lit with directional and point-light shadows cast and received, the scene-effect attachments with a moving terrain, the CPU selection path on native OpenGL when available, and refusal of invalid settings.

## Differences From TypeScript
- **Ownership.** Resources release on `Drop`, so there is no `destroy()` or constructor rollback to call, and no `ready()`: wgpu validates pipelines at creation. `TerrainAsset` is a cheaply cloned shared handle with accessors.
- **Loading.** `load_terrain` reads a manifest and its height file from disk; browsers embed or fetch both files and call `parse_terrain`.
- **Typed inputs.** `TerrainDiagnostic` replaces the numeric mode, `TerrainView` uses `DVec3`, `reconcile_terrain_edges` fails for a detail list of the wrong length, and `sample_morphed_height` returns `None` for an unknown patch. `TerrainInstance` settings are public fields validated when the frame is prepared rather than when they are set.
- **Patch records.** One 256-byte-stride buffer bound at dynamic uniform offsets replaces the WebGPU storage descriptors plus per-draw index uniforms and the WebGL per-patch uniform buffers, so the CPU path uploads 128 + 256 bytes per patch (65,664 for Dragon Pit) instead of 128 + 32. GPU selection follows device capability (compute and indirect draws on a non-GL backend), not the WebGPU device type.
- **Scene lighting.** Terrain uses the Rust renderer's single lit pipeline and its cascaded directional and point-light shadows, which also run on WebGL 2; the TypeScript scene path lights terrain through its batched shadow passes on WebGPU only. The surface attachment reads the frame camera, and terrain roughness is fixed at one as in TypeScript. Pipelines are shared by every terrain instance.
- **Statistics.** `TerrainStats::record_milliseconds` (CPU time recording the standalone pass) replaces the example's submit time, because the shell submits once per frame after the UI.
- **Scene instances.** Rust scenes are flat, so a terrain has a `Transform` rather than a node hierarchy, and adapters are keyed by `TerrainInstanceId` (rebuilt when the instance's asset changes) rather than by object identity.
- **Example.** Assets are embedded instead of fetched; the pixel-ratio cap of 2 is not applied (the shell owns backing resolution); the pan modifier is Command or Alt on every platform; the draw-path switch and the source notice are additions; page readiness markers for the TypeScript capture script are omitted.
