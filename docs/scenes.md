# Mesh Assets and Rendering Scenes
Joy Engine 0.8.0 owns shared mesh assets, persistent presentation instances and frame preparation. LUNA and MAGIC COLORS use this layer; their simulation and cosmetic effect lifetimes remain project-owned.

## Create an asset and instance
```js
import { RenderScene, SceneRenderer, OrbitCamera } from 'joy-engine';
import { createFormMeshAsset } from 'joy-engine/form';
import heroSource from './assets/hero.form?raw';

const asset = createFormMeshAsset(heroSource);
const scene = new RenderScene();
const hero = scene.createMesh(asset, {position: [0, 0, 0]});
const camera = new OrbitCamera();
const renderer = new SceneRenderer(canvas, {camera});
await renderer.initialize();

// Update presentation from gameplay; units are world units and radians.
hero.position[0] = player.x;
hero.position[2] = player.z;
hero.rotation[1] = player.heading;
renderer.render(scene, {timeSeconds: world.time, lights: [], ambient: 0.5});

renderer.destroy();
scene.destroy();
```

`MeshAsset` copies and freezes indexed local-space geometry. Normals, UVs, colors, alpha, material assignments and materials remain available on `asset.geometry`. The FORM producer also preserves named outputs, parameters, point metadata and project metadata under `asset.geometry.metadata`. Local bounds are available as `asset.bounds`. Shared assets contain no device-specific resources; removing an instance or destroying a scene cannot invalidate an asset used elsewhere.

`new MeshAsset({positions, indices, ...})` accepts engine primitives and other geometry producers. `meshBox()` from `joy-engine/form` already supplies this indexed representation. Recompile an asset to regenerate visuals, then assign the replacement asset to its existing instance; instance identity and presentation transforms remain intact.

## Instance transforms and ownership
Instances own mutable `position`, `rotation`, `scale`, `tint`, `opacity`, `visible`, and `pass` values. Positions use Y-up world coordinates. Rotations are XYZ Euler radians applied X, then Y, then Z; positive Y rotation preserves the prototypes' heading convention. Scaling precedes rotation and translation. Nonuniform, negative, and zero scales are supported by the vertex-color adapter.

`instance.set(options)` validates and copies supplied values; omitted fields retain their values. Direct array edits are supported and must retain three finite components. Opacity is between zero and one. `pass` is `auto`, `opaque`, or `transparent`; auto routes instances with vertex alpha or opacity below one to transparency. Explicit transparent instances preserve unlit projectile behavior even at full opacity.

`scene.remove(instance)` removes that view. `scene.clear()` removes all instances while leaving the scene and its collections reusable. `scene.destroy()` is idempotent and prevents further creation. Scenes hold no gameplay authority and do not save or destroy simulation entities. The renderer borrows the scene and camera; dispose each owner explicitly.

## Synchronize a collection
```js
const actors = scene.createCollection();

function synchronizeActors(world) {
  actors.beginUpdate();
  for(const actor of world.actors) {
    actors.set(actor.id, assets[actor.kind], {
      position: [actor.x, actor.y, actor.z],
      rotation: [0, actor.heading, 0],
      tint: actor.frozen ? [0.6, 1.2, 1.8] : [1, 1, 1]
    });
  }
  actors.endUpdate();
}
```

A key returns the same instance across updates. Missing keys are removed at `endUpdate()`, so dead or despawned entities cannot grow the scene indefinitely. Keys may be stable IDs or simulation record references; avoid array indices when removing/reordering actors. Collections borrow record keys until removed and never modify them. Static scenery can use `scene.createMesh()` once instead of joining a collection update.

## Frame preparation
`SceneRenderer` owns backend selection, standard shaders, canvas resizing, postprocessing, instance transforms, conservative frustum culling, render queues, transparency sorting, light uniforms and GPU cleanup. It is the default composition root for games and editor viewports and uses the existing WebGPU-first/WebGL 2 fallback. `backend`, `colorFormat`, `ready` and `stats` expose diagnostics without access to device resources. `getDiagnostics({memory, resources})` copies requested luma counters on demand, plus this scene’s last visible/culled counts; it returns null before initialization or after disposal. Allocation/resource counters cover all luma devices in the JavaScript realm, not only this scene. Missing counters remain null and the snapshot is caller-owned. Postprocessing is enabled by default, accepts the existing resolved profile/configuration, and may be disabled explicitly with `postProcessing: false` for focused diagnostics. `setPostProcessingConfig()` updates it after initialization.

Unchanged instance geometry is cached in scene-owned weak references. The adapter still assembles and uploads CPU triangle batches. Retained instances do not imply hardware instancing or persistent GPU meshes in this release.

Opaque meshes use the existing flat, double-sided vertex-color lighting. Transparent meshes are unlit. Authored material/normal/UV channels are preserved but this adapter does not evaluate PBR materials or textures. Projects should select an appropriate render adapter before relying on those channels visually. Transparency is globally centroid-sorted and retains the usual intersecting-surface limitations. Lights do not cast shadows; the existing 32-light maximum applies.

For bespoke procedural feedback and existing particle code, `render()` accepts borrowed `opaqueTriangles` and `transparentTriangles`. These are world-space XYZ/RGBA triangles and participate in the shared frame, lighting and transparency ordering. This escape hatch does not transfer effect lifetime management into the engine. LUNA and MAGIC COLORS keep their project-authored rings, slash/bolt shapes, event mapping and particle lifetime logic in this pass.

## Camera queries
`renderer.screenRay(clientX, clientY)` uses client CSS pixels and the actual camera matrix, including backend clip-depth conventions. `renderer.pickGround(clientX, clientY, height)` intersects a horizontal world plane. `renderer.project([x, y, z])` returns canvas-local CSS pixels or null behind the camera/when the canvas has no size. Device pixel ratio only changes drawing-buffer density, never view or picking scale. Low-level `screenRay`, `projectToViewport`, and `intersectRayPlane` functions are also public for custom camera integrations.

## Verification
The focused tests are `joy-engine/test/mesh-scene.test.mjs` and `scene-frame.test.mjs`. Run them with the repository raw-asset loader. Project build/capture commands remain `npm run build:luna`, `npm run capture:luna`, `npm run build:magic-colors`, and `npm run capture:magic-colors`. Use `-- --built` on capture commands for production previews.

### Results for this branch
Root `npm run check`, all 300 tests and `npm run build` passed. The seven focused scene tests cover ownership, attribute preservation, collection pruning, transforms/cache updates, frustum culling, CSS projection, frame submission and late initialization cleanup. An independent code review found no blocking issues.

LUNA's complete development capture passed, including combat, progression, pause, responsive layouts and unsupported-GPU handling. MAGIC COLORS' complete production capture passed, including reactions, reset, narrow layouts, touch input, production diagnostic silence and unsupported-GPU handling; its gallery preview was refreshed from that run.

The broad `npm run validate` reached browser captures after passing checks/tests/build, but BLAST GARDEN could not find its default Playwright browser. That broad capture run was stopped. LUNA's production capture timed out during a screenshot after startup, without browser application errors. Software Chromium screenshots were inspected; they are not GPU performance measurements. Safari, Edge and hardware WebGPU remain unverified. No dependency versions were changed to work around browser availability.
