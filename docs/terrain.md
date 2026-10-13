# Resident CLOD terrain
Joy exposes `loadTerrain`, `decodeTerrain`, `sampleTerrain`, `selectTerrain`, `reconcileTerrainEdges`, `sampleMorphedHeight` and `TerrainRenderer`. The [terrain example guide](../examples/terrain/README.md) specifies asset version 1, offline conversion, units/orientation, render-pass ownership, backend behavior and collision/road/environment integration boundaries. This pass retains indexed geometry and never expands a full landscape into the existing per-frame scene triangle stream.


## Scene Integration
Use `RenderScene.createTerrain(asset, options)` for scene-owned resident terrain. The [scene graph guide](scenes.md) covers parenting, shadow policy, LOD settings, and attachment limitations. `TerrainRenderer` remains available for standalone indexed rendering into a borrowed pass. Native scene terrain shares the renderer's light maps and depth attachment; it uses linear scene lighting before the normal display resolve. Runtime height editing and streamed residency are tracked in [#61](https://github.com/trentpolack/joy-engine/issues/61).
