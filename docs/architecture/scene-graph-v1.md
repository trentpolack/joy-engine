# Scene Graph V1 Design

The goal is one scene-owned representation for geometry, transforms, visibility, bounds, and shadow participation. Registering solid geometry should make it eligible for the engine's ordinary depth and shadow passes without a game wiring each pass independently. This implements the scene foundation discussed in issue #60; scalable shadow scheduling, shadowed volumetric integration, and editable/streamed landscapes remain the larger workstreams in #60 and #61.

## Scene Ownership

`RenderScene` owns a hierarchy of `SceneNode` objects. Mesh instances are geometry-bearing nodes; transform-only nodes group objects. Nodes expose local XYZ position, XYZ Euler rotation in radians, scale, and visibility. World transforms compose full affine matrices, preserving shear caused by rotated children under nonuniformly scaled parents. Direct array edits remain supported. Parenting preserves the local transform, rejects cycles and cross-scene ownership, and is atomic on failure. Removing a node removes its subtree; assets remain borrowed and reusable. Destroying the scene invalidates nodes and releases references, not borrowed assets or renderer-owned GPU resources.

## Geometry And Frame Preparation

All scene geometry shares stable node identity, local bounds, world transforms, material coverage, and pass policy. Existing mesh/FORM consumers retain `createMesh`, collections, and instance mutation. Frame preparation first traverses the scene once into reusable world snapshots, then derives camera-visible geometry and shadow candidates independently. Main-camera culling must never discard a caster that can affect visible receivers. Native resident geometry uses the same scene ownership and pass policy while retaining its GPU representation. Terrain must not be expanded into per-frame CPU triangles.

Opaque geometry casts and receives shadows by default on WebGPU. Transparent geometry retains the existing unlit blending behavior and does not become an opaque blocker. Per-node `castsShadow` and `receivesShadow` are explicit controls. Hidden ancestors hide their subtree. Material alpha coverage must remain consistent with the existing PBR masked caster shader. WebGL 2 keeps illumination and depth, with geometric shadows reported unavailable.

## GPU Ownership

One frame owner prepares shadow maps from all participating geometry. Colored and PBR receivers share those maps instead of preparing duplicated copies. Geometry adapters own shaders/buffers appropriate to their representation, while the renderer owns pass sequencing and device lifetime. Geometry revisions and removal must invalidate cached world data and release detached GPU resources. Scene effects may only consume attachments that a geometry adapter actually supplies; unsupported native normal/motion combinations must report a specific capability error rather than silently producing incomplete occlusion.

## Validation

Behavioral regressions cover hierarchical motion, inherited visibility, cycles, cross-scene parenting, subtree removal, retained shared assets, cache invalidation, off-camera casters, per-node shadow controls, and shared colored/PBR shadow ownership. Browser evidence must include nested geometry, camera movement, mixed colored/PBR shadows, and native terrain participation when enabled. Existing engine/workspace checks and tests, affected builds, and available browser integration runs remain required. Record baseline failures and unavailable browser/hardware evidence explicitly.
