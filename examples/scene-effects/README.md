# Scene effects example
This developer workspace is excluded from the published site and automatic release captures. It consumes the public `joy-engine` scene API without importing luma directly. Its deterministic room demonstrates depth-aware blur, SSAO or GTAO, SSGI, outlines, TAA, motion blur, SSR, height fog, clustered volumetric lighting, adaptive exposure and existing HDR bloom. Start from the repository root:

```sh
npm run dev --workspace @joy-engine/example-scene-effects
npm run check --workspace @joy-engine/example-scene-effects
npm run build --workspace @joy-engine/example-scene-effects
npm run capture --workspace @joy-engine/example-scene-effects
npm run capture --workspace @joy-engine/example-scene-effects -- --built
```

Append `?backend=webgl` or `?backend=webgpu` to choose a backend. Explicit WebGPU selection reports adapter failure instead of silently falling back. Automatic selection prefers WebGPU and falls back to WebGL. Unsupported hardware and shader failures appear in the page. The capabilities panel shows the renderer's device report, including its HDR attachment choice.

## Scene and controls
The open room contains saturated red and green walls for screen-space color bleed, blocks and corners for contact occlusion, thin vertical slats for temporal antialiasing, a floor with scene roughness 0.18 and a blue block with roughness 0.05 for screen-space reflections. The ceiling strip uses linear RGB `[12, 7, 2]`; bloom has threshold 1, so highlights have real HDR radiance. The 27 deterministic point lights illuminate the room and supply the clustered volumetric pass. Scene roughness is a screen-space reflection classification, not an authored PBR material. Height fog is the upstream stylized screen-height/depth approximation; clustered lighting uses world-space volumetric scattering. The camera uses near/far clips 0.5/100 world units to preserve useful depth precision in this small room.

CONFIG enables each effect independently. SSAO and GTAO are mutually exclusive; enabling either switches off the other. Low, medium and high quality adjust bounded sample counts and intermediate resolution without enabling additional effects. All off restores the comparison baseline; Combined enables GTAO, SSGI, TAA, SSR, clustered lighting, adaptive exposure and bloom. Drag or scroll in the viewport to orbit or zoom. The example requests backing pixel ratio 1 so capture resolution is reproducible.

Simulation starts frozen at 2 seconds while frames continue rendering to accumulate temporal history. Moving object and Camera motion enable deterministic sine motion. Uncheck Fixed simulation time to advance using the animation-frame clock, or use Time and Step 1/60 s to select reproducible motion frames. Camera cut, Replace scene, Reset history and Resize test exercise history invalidation. Scene replacement changes the wall colors and explicitly destroys the old scene. The owner releases RAF, DOM subscriptions, room instances and GPU resources on teardown and Vite replacement.

## Capture API and evidence
The typed `window.sceneEffectsExample` development/capture API is defined by `SceneEffectsExampleApi` in `site/src/main.ts`. It exposes effect/quality/bloom/motion/time controls, `baseline`, `combined`, `cameraCut`, `replaceScene`, `resetHistory`, `resizeTest`, `snapshot`, `resetMetrics`, `sample` and idempotent `dispose`. Settings use complete replacement through `SceneRenderer.setSceneEffectsConfig`; per-frame inputs include seconds, delta seconds and explicit history reset.

The browser script starts an isolated random-port Vite server and a temporary Playwright browser. On macOS it selects installed Google Chrome when available, with headed rendering by default. Set `CAPTURE_EXECUTABLE` to choose another Chromium executable, `SCENE_EFFECTS_BACKEND=webgl` or `webgpu` for one backend, `SCENE_EFFECTS_HEADLESS=1` for headless, or `SCENE_EFFECTS_HEADED=1` to force headed elsewhere. Safari and Edge are not covered by this Chromium harness.

Every available backend captures baseline, every individual effect, bloom, combined low/medium/high, motion, camera cut, replacement, step/reset, resize, repeated toggling and effects disabled again. The script checks shader/page errors, SSAO/GTAO exclusivity, nonempty timing samples and production log silence. It retains screenshots, an HTML gallery and `report.json` beneath ignored `artifacts/captures/`. The JSON includes hardware/browser/backend, capabilities, logical/backing resolution, all settings, camera pose and two separate timing distributions. Appearance must be inspected from the screenshots; these measurements are not a GPU benchmark.

CPU render submission is `performance.now()` around `SceneRenderer.render`, including CPU frame preparation and command submission. RAF cadence is the difference between browser animation-frame timestamps. GPU execution time is explicitly unavailable. Frozen simulation time does not freeze cadence measurement; neither measure is relabeled as GPU time. Adaptive exposure still receives deterministic delta seconds while the scene clock is frozen, allowing repeatable convergence after a fixed frame count.
