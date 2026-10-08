# DCC interchange API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## loadGlbMesh
Import from `joy-engine`. [Source](../../src/dcc/glb-mesh-loader.ts)

```ts
async function loadGlbMesh(url: string, meshName?: string): Promise<IndexedMesh3D>
```
Load one indexed triangle mesh from a binary or JSON glTF 2.0 asset. Linked buffers in JSON glTF assets are resolved relative to the asset URL. Returns copied model-space data; callers own projection, materials, and lighting.

- **param** url
- **param** [meshName] Exact glTF mesh name to decode.
- **returns** Model-space data owned by the caller.


## parseGlbMesh
Import from `joy-engine`. [Source](../../src/dcc/glb-mesh-loader.ts)

```ts
async function parseGlbMesh(buffer: ArrayBuffer, meshName?: string): Promise<IndexedMesh3D>
```
Decode one indexed triangle mesh from in-memory binary glTF 2.0 data. A mesh name can select one object from exporters that retain other scene objects. loaders.gl owns container, buffer, accessor, sparse-data, and stride decoding. Unindexed triangles receive sequential indices. Scene/node transforms, materials, animation, and additional primitives are intentionally not interpreted.

- **param** buffer
- **param** [meshName]
- **returns** Copied model-space data owned by the caller.
