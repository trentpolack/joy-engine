# Scene effects

`SceneRenderer` accepts individual `sceneEffects` opt-ins and retains the existing HDR display/tone-mapping/bloom path. Defaults remain disabled. The Level viewport and development Vehicle Playground expose temporary controls; changes do not rewrite level assets. The isolated [Scene Effects Lab](../examples/scene-effects/README.md) provides deterministic individual and combined comparisons.

```ts
const camera = new OrbitCamera();
camera.nearClip = 0.5;
camera.farClip = 100;
const renderer = new SceneRenderer(canvas, {
  camera,
  sceneEffects: {gtao: true, taa: true, motionBlur: {strength: 0.5}},
  postProcessing: {bloomStrength: 0.2}
});
await renderer.initialize();
renderer.render(scene, {timeSeconds, deltaTimeSeconds, lights});
// Complete replacement, atomically validated; unspecified effects become disabled.
renderer.setSceneEffectsConfig({ssao: true, outlines: true});
renderer.resetHistory(); // teleports/content edits retaining instance identities
renderer.destroy(); // scene and camera are borrowed
```

## Inputs and ownership

Opaque flat-lit mesh color/depth is rendered offscreen. View-space face normals encode `normal * 0.5 + 0.5`; alpha contains perceptual `MeshInstance.sceneRoughness` (default 1, matte). This metadata enables SSR without changing authored materials or converting games to PBR. Exact opaque depth is borrowed by the normal pass. Object identity/topology correspondence retains previous world positions; velocities are signed current-minus-previous top-left UV. TAA and blur use unjittered velocity, while GTAO/SSGI/SSR receive jitter-inclusive velocity. Camera and raster matrix histories are separate to avoid double jitter correction.

Only opaque meshes participate in these effects. Transparent feedback/particles render afterward against the original opaque depth and then enter HDR display processing. Transient opaque triangle arrays lack stable identity; they conservatively reset temporal history rather than invent motion correspondence.

Owned passes, textures, framebuffers and clustered buffers are released on disposal. Replacement graphs are constructed before replacing the working graph. Histories reset on resize, hidden/resumed viewport, camera replacement/cut, clip/FOV changes, scene replacement, topology/visibility discontinuities, explicit reset, time rewind/gap over 0.25 seconds, toggling and failed submission. Callers must explicitly reset after discontinuous changes that retain identities; continuous object/camera motion uses real previous positions/matrices.

## Supported combinations

| Effect | Contract / limitation |
| --- | --- |
| Depth-aware blur | Bilateral opaque color/depth blur. |
| SSAO | Perspective only; depth reconstruction or supplied view normals. Mutually exclusive with GTAO. |
| GTAO | Perspective horizon AO with temporal depth rejection. Upstream composites against full opaque color; ambient-only separation is unavailable in this renderer. |
| SSGI | Colored screen-space diffuse bounce; offscreen/occluded sources cannot contribute. |
| Outlines | Depth/normal edges with independent opt-in. |
| TAA | Default velocity reprojection supports object and camera motion. Explicit `reprojection: 'camera'` assumes static geometry; use velocity for moving scenes. |
| Motion blur | Unjittered object/camera velocity, with bounded sample count. |
| SSR | Screen-space reflections classified by scene roughness; offscreen reflections are unavailable. |
| Height fog | Upstream compact screen-height/raw-depth approximation. Temporal accumulation is disabled because it has no camera reprojection. It is not world-space height integration. |
| Clustered lighting | Participating-media scattering with directional radiance and actual view-space point lights. Conservative CPU cluster lists feed GPU integration, capped at 32 lights / 8 per cluster; overflow is reported by `sceneVolumeStats`. No shadow-map occlusion is supplied. |
| Adaptive exposure | GPU luminance meter/adaptation driven by delta seconds. No CPU readback. |
| HDR bloom | Existing multiscale display bloom, independently controlled after scene effects. |

The shared adapter supports perspective `OrbitCamera`. Lower-level `SceneEffects` validates its camera/input contract and rejects unsupported combinations. `postProcessing: false` cannot be combined with scene effects. WebGPU floating-point attachments and precise filterable 32-bit temporal depth history are required for relevant passes. Unsupported configurations report specific capability failures. WebGL preserves the base scene/display path and reports scene effects unavailable; it does not synthesize inputs. Explicit WebGPU selection reports adapter failure, while automatic device selection retains its existing WebGL fallback.

Upstream `ShaderPassRenderer` can throw during its own constructor before returning an owner. Completed stages are cleaned up; hidden allocations inside a partially constructed upstream stage cannot be individually reclaimed on a live-device configuration failure. Initial renderer creation destroys its device on failure. This upstream limitation remains; device loss itself is not automatically recovered.

## Verification

The focused tests cover configuration bounds, inputs, matrix/jitter conventions, object identity correspondence, resize/reset, failure rollback, attachment ownership and clustered light bounds. Run `npm run check`, `npm test`, `npm run validate`, and the example capture commands in its README. Root release capture intentionally skips command-line examples, so Scene Effects Lab requires its own capture invocation.

Native Mac evidence and timings are recorded in [scene-effects-verification.md](scene-effects-verification.md). CPU submission includes scene preparation and command submission. RAF cadence describes browser scheduling. Neither is measured GPU execution time.
