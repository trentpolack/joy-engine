# Scene Effects Example
A deterministic Rust room for inspecting depth-aware blur, SSAO or GTAO, SSGI, outlines, TAA, motion blur, SSR, fog, volumetric lighting, adaptive exposure, and HDR bloom.

```bash
npm run dev --workspace @joy-engine/example-scene-effects
npm run build --workspace @joy-engine/example-scene-effects
npm run test --workspace @joy-engine/example-scene-effects
npm run capture --workspace @joy-engine/example-scene-effects -- --built
```

Native development runs the Cargo example; browser output lives in `dist/` and publishes at `/examples/scene-effects/`. Use `backend=webgpu` or `backend=webgl` in browser queries. Scene effects require WebGPU or a capable native backend; WebGL 2 reports them unavailable while retaining scene rendering, shadows, and HDR bloom.

## Scene and Controls
Colored walls reveal bounce, blocks and corners reveal occlusion, slats expose temporal stability, and low-roughness surfaces reveal reflections. HDR lighting and deterministic point lights supply actual scene inputs. Camera clips are 0.5/100 world units.

The shared `SceneEffectsControls` provides independent toggles, exclusive SSAO/GTAO selection, All off/Combined presets, bounded quality settings, and bloom. EFFECT GUIDE explains limitations. Fixed time, stepping, object/camera motion, camera cuts, scene replacement, and history reset support repeatable inspection. Drag and wheel orbit/zoom.

## Validation
`tests/integration/example_scene_effects.rs` drives the real application headlessly, including shadow/material fixtures; `JOY_RENDER_CAPTURE_DIR` retains GPU captures. Browser captures smoke-test WebAssembly initialization and rendered images. Inspect temporal motion interactively as well as still captures. Statistics report CPU encoding and frame cadence, not GPU execution time. See [scene effects](../../docs/scene-effects.md).
