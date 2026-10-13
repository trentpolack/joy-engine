# Scene Graph And Geometry
`RenderScene` owns a hierarchy of presentation nodes. Meshes and resident terrain share transforms, visibility, bounds, and pass policy. Gameplay state stays with the game; authored assets remain reusable across scenes.

## Create A Hierarchy
```ts
import {MeshAsset, RenderScene, SceneRenderer} from 'joy-engine';

const scene = new RenderScene();
const group = scene.createNode({position: [4, 0, 0]});
const mesh = scene.createMesh(new MeshAsset({positions, indices}), {
  parent: group,
  position: [0, 1, 0],
  castsShadow: true,
  receivesShadow: true
});
const renderer = new SceneRenderer(canvas);
await renderer.initialize();
group.rotation[1]+= 0.1;
renderer.render(scene, {lights, timeSeconds});
```

Nodes expose local `position`, `rotation`, and `scale` arrays, plus `visible`. Positions use Y-up engine units; rotations are XYZ Euler radians, applied X, then Y, then Z after scaling. Parents compose full affine transforms, including shear from rotated children under nonuniform scale. Negative and zero scales are supported. Direct array edits must retain three finite components. `set(options)` validates before changing state.

`node.setParent(parent)` preserves the local transform. Pass `null` to make a root. Cycles, foreign scenes, and removed parents are rejected without changing the hierarchy. A hidden ancestor hides its descendants. `worldTransform` is a read-only 12-number column-major affine snapshot: three basis columns followed by translation. `worldVisible` and `worldRevision` reflect ancestor changes. Frame preparation updates world transforms once, before deriving geometry packets.

`scene.remove(node)` removes its complete subtree and invalidates its nodes. `clear()` leaves the scene and keyed collections reusable; `destroy()` is idempotent and prevents further creation. Assets are borrowed, so neither operation destroys shared mesh or heightfield data. Renderers release detached terrain GPU adapters on their next frame, or immediately when destroyed. Dispose the renderer before releasing its device.

## Mesh Assets And Collections
`MeshAsset` copies and freezes indexed local geometry and exposes local bounds. FORM assets retain normals, UVs, colors, alpha, materials, named outputs, and metadata. Assigning a replacement mesh asset preserves instance identity. The standard scene mesh adapter evaluates vertex colors; authored PBR materials remain available to the low-level material adapter and are not automatically interpreted by `createMesh`.

Meshes add `tint`, `opacity`, `sceneRoughness`, and `pass`. `pass: 'auto'` sends alpha-bearing assets or opacity below one to transparency. Explicit `opaque` and `transparent` policies remain available. Transparent meshes retain unlit, globally centroid-sorted blending and do not become solid shadow blockers.

```ts
const actors = scene.createCollection();
actors.beginUpdate();
for(const actor of world.actors) {
  actors.set(actor.id, assets[actor.kind], {
    parent: group,
    position: [actor.x, actor.y, actor.z]
  });
}
actors.endUpdate();
```

Stable keys retain instances; missing keys are removed at `endUpdate()`. Collections recognize nodes removed through their ancestors. Neither keys nor simulation records are modified by the scene.

## Native Terrain
```ts
import {loadTerrain} from 'joy-engine';
const asset = await loadTerrain('/landscape/terrain.json');
const ground = scene.createTerrain(asset, {
  parent: group,
  tolerance: 2,
  targetRadius: 12
});
```

Terrain nodes share the hierarchy and default shadow/depth participation while retaining resident indexed GPU geometry. Camera movement updates compact selection data; it never expands a landscape into the mesh triangle stream. Terrain uses the same morphed surface for color, depth, and light-space shadow views. LOD projection scale uses CSS pixels, and affine transforms conservatively adjust local selection. `targetRadius` is in world units around the camera target; `diagnostic` selects material (0), LOD colors (1), or grid (2).

The adapter supplies color and depth, with shared point/directional lighting. Native normal/roughness and motion attachments are not implemented: WebGPU effects requiring these reject the frame with a specific attachment error. Depth-reconstructed effects remain available. WebGL keeps the existing source-preserving effects fallback. Terrain editing, streaming, collision updates, and independently selected shadow LOD remain later work in [#61](https://github.com/trentpolack/joy-engine/issues/61).

## Shared Pass Participation
Opaque mesh and terrain nodes cast and receive geometric shadows by default on WebGPU. `castsShadow = false` removes a node from light views; `receivesShadow = false` preserves direct illumination without applying shadow visibility. Camera culling and caster selection are independent: an off-camera object can still shadow a visible surface. One frame owner records shadow maps for colored, PBR, and native terrain adapters; their receivers borrow the same bindings. PBR masked casters retain texture-aware alpha rejection. WebGL 2 retains lighting and depth while reporting geometric shadows unavailable.

This hierarchy is not a spatial acceleration structure. Mesh world vertices remain cached on the CPU and uploaded in batches; terrain retains native GPU buffers. The graph does not yet provide GPU mesh instancing, streamed world partitioning, occlusion culling, or shadowed volumetrics. Those scaling stages are tracked in [#60](https://github.com/trentpolack/joy-engine/issues/60).

`SceneRenderer` owns backend selection, CSS-aware resizing, camera culling, draw sequencing, atmosphere, post-processing, and GPU cleanup. `backend`, `colorFormat`, `ready`, `stats`, and `getDiagnostics()` expose state. Global luma allocation counters are distinct from per-renderer shadow work. Frame `opaqueTriangles` and `transparentTriangles` remain borrowed world-space XYZ/RGBA escape hatches for procedural feedback and particles.

## Camera Queries And Validation
`screenRay`, `pickGround`, and `project` use CSS pixels and the camera's backend clip convention. Pixel ratio changes backing density without changing world framing.

Core regressions live in `test/mesh-scene.test.mjs`, `test/scene-frame.test.mjs`, `test/pbr-triangles.test.mjs`, and `test/terrain-renderer.test.mjs`. The Scene Effects workspace provides `test:scene-graph-browser` for mixed hierarchy/terrain participation and `test:shadows-browser` for material alpha coverage and light batching. Both accept `CAPTURE_EXECUTABLE_PATH` and JSON `CAPTURE_ARGS` for an available Chromium installation. Captures are appearance evidence, not hardware performance measurements.
