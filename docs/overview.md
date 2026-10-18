# Engine Architecture
Joy Engine is a workspace of system crates behind the `joy-engine` facade crate. The [runtime guide](rust.md) describes its frame phases and platform setup; [dependency layers](architecture/dependency-layers.md) defines allowed imports.

## Rendering Responsibilities
`joy-renderer`'s `renderer.rs` orchestrates targets, frame preparation, pass order, and resource lifetime. Material passes own their shader interface, vertex layout, textures, blend/depth state, and preparation. Lighting, environment, shadows, scene effects, and display processing remain shared renderer stages. A new material should participate in those stages rather than create a separate renderer.

`RenderScene` retains mesh and terrain instances and optional parent nodes. `MeshAsset` handles share immutable geometry; the renderer retains uploaded resources. Conservative instance culling preserves offscreen shadow casters. Terrain keeps its heightfield and indexed grid resident, selects continuous LOD on the GPU when supported, and uses CPU selection on GL backends. Transparent geometry sorts back to front and retains the usual intersecting-surface limitations.

The renderer supports Lit, PBR, and Unlit surfaces, atmosphere, directional cascades, point-light shadows, image-based lighting, and HDR display processing. Backend capability checks remain centralized. WebGPU is preferred in browsers, WebGL 2 is the fallback, and WebGL 1 is not supported.

## Simulation and Content
`World` owns entity and physics simulation without a renderer. `LevelRuntime` instantiates authored documents; `LevelScene` and `AssetScene` belong to rendering and synchronize presentation separately. FORM compiles procedural source to plain geometry. Particle definitions are shared while each effect owns its simulation and cosmetic random stream. Games retain rules, camera policy, tuning, and authored assets.

`ChunkStructure` queues damage until `commit`, publishing one revision for geometry and collision. Optional support rules detach unsupported chunks in that same revision. Physics mutation batches keep collision changes ordered. These are reusable mechanisms; project-specific damage rules and destruction presentation stay with the game.

## Ownership and Diagnostics
Rust owners release resources through `Drop` or explicit lifecycle operations where required. Public contracts document coordinates, time units, mutation, failure, and owned versus borrowed data. Development diagnostics report measured values without treating frame cadence or CPU submission as GPU execution time. See [coding standards](coding-standards.md), [scenes](scenes.md), and [authoring tools](rust-authoring.md).
