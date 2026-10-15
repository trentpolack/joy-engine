# Dependency Layers
Joy Engine source follows a one-way dependency model so simulation remains runnable without a DOM, canvas, or GPU:

1. **Foundation** contains shared data and algorithms.
2. **Simulation** contains environment, entity, gameplay, and physics state.
3. **Content** contains asset loading, compilation, and scripting.
4. **Rendering** contains GPU presentation and render features.
5. **Platform** contains browser, development, and frame-loop integrations.

Higher layers may import the allowed lower layers recorded in `architecture/layers.json`. The checker reads static imports, re-exports, and literal dynamic imports, and rejects new upward edges. Existing upward edges are named individually as migration debt; removing one also requires removing its now-stale exception. This makes the allowlist shrink during the renderer migration rather than silently becoming a permanent architectural bypass.

Backend packages have owners too. `backendPackages` names the layers allowed to import each one; luma.gl belongs to the rendering layer, so simulation, content, and platform modules reach the GPU through rendering owners. The editor and examples listed in `consumerRoots` may not reference a backend package at all, including JSDoc `import()` types and Vite dependency lists; they use `joy-engine` exports such as `GpuDevice` and `createEngineDependenciesPlugin()`.

The Rust engine in `crates/` follows the same model with one crate per layer. `rustCrates` assigns each crate its layer, `rustFacadeCrate` names `joy-engine` (which may depend on every layer and is not a dependency of any), and `rustBackendCrates` limits wgpu to rendering and platform, rapier to simulation, and winit and egui to platform. `scripts/check-rust-layers.mjs` reads the crate manifests during `npm run check`; Cargo itself rejects cycles. The Rust crates have no recorded exceptions: level presentation lives in the rendering layer and FORM returns renderer-free geometry, which retires the migration debt listed above for the TypeScript engine.

`World` is the headless composition boundary for entities and physics. Rendering observes simulation through a separate application integration and must not become owned by `World`.
