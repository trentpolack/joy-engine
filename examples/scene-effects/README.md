# Scene Effects Example
A deterministic Rust room for inspecting temporal antialiasing, motion blur, adaptive exposure, volumetric fog and lighting, and HDR bloom.

```bash
npm run dev --workspace @joy-engine/example-scene-effects
npm run build --workspace @joy-engine/example-scene-effects
npm run test --workspace @joy-engine/example-scene-effects
npm run capture --workspace @joy-engine/example-scene-effects -- --built
```

Native development runs the Cargo example; browser output lives in `dist/` and publishes at `/examples/scene-effects/`. Use `backend=webgpu` or `backend=webgl` in browser queries. Volumetric fog requires compute shaders (WebGPU or a native backend); WebGL 2 reports it unavailable while temporal antialiasing, motion blur, adaptive exposure, and HDR bloom run on float render targets.

## Scene and Controls
Slats expose temporal stability, the moving block and camera sway expose motion blur, bright strips drive bloom and exposure, and blocks cast volumetric shadows through the fog lit by deterministic point lights. Camera clips are 0.5/100 world units.

The shared `SceneEffectsControls` provides independent toggles, All off, Cinematic, Atmosphere, and Combined presets, bounded quality settings, and bloom. EFFECT GUIDE explains limitations. Fixed time, stepping, object/camera motion, camera cuts, scene replacement, and history reset support repeatable inspection. Drag and wheel orbit/zoom.

## Validation
`tests/integration/example_scene_effects.rs` drives the real application headlessly, including shadow/material fixtures; `JOY_RENDER_CAPTURE_DIR` retains GPU captures. Browser captures smoke-test WebAssembly initialization and rendered images. Inspect temporal motion interactively as well as still captures. Statistics report CPU encoding and frame cadence, not GPU execution time. See [scene effects](../../docs/scene-effects.md).
