# Resident CLOD Terrain
The engine's `rendering::terrain` module loads version-1 terrain assets, samples gameplay heights, selects continuous LOD, and draws through retained `TerrainInstance`s or `TerrainRenderer`.

Heightfields and the reusable grid stay resident. Capable backends select LOD and indirect draws on the GPU; GL backends use the same policy on the CPU. Scene terrain shares lighting, shadows, surface attachments, and motion data with meshes. Gameplay sampling always uses the finest surface rather than camera-dependent LOD.

See the [terrain implementation guide](rust-terrain.md) for API and backend contracts and the [terrain example](../examples/terrain/README.md) for the asset format, conversion, coordinates, and controls.
