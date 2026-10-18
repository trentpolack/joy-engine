# Dependency Layers
Joy Engine uses a one-way dependency model so simulation runs without a window, canvas, or GPU:

1. Foundation contains shared data and algorithms.
2. Simulation contains environment, entity, gameplay, and physics state.
3. Content contains asset parsing, compilation, and script information.
4. Rendering contains GPU presentation and effects.
5. Platform contains the application shell, input, audio, storage, UI, development tools, and the authoring kit, with their browser backends.

`architecture/layers.json` records each layer's engine crates, the facade modules that gather them, and the allowed edges. `npm run check` runs `scripts/check-boundaries.mjs`, which reads the workspace manifests through `cargo metadata` (an engine crate may depend only on crates in its own or a lower layer, backend crates only from their owning layers, and test support only as a dev-dependency) and checks the facade modules' `crate::<module>` paths, grouped imports, documentation paths, and named engine crates against their top-level source file or folder. Cargo itself keeps a crate from naming anything it does not declare. Internal code uses crate- and module-qualified paths; the re-exports in `src/lib.rs` are the consumer-facing API.

Backend crates also have owners: wgpu belongs to rendering and platform, rapier to simulation, and winit and egui to platform. Games, tools, and examples depend on `joy-engine`, using its re-exported crates and UI contracts rather than system crates or backend dependencies. The engine never depends on those consumers.

`World` is the headless composition boundary for entities and physics. The renderer depends only on foundation data and particle documents; `joy-scene` (`LevelScene`, `AssetScene`, the FORM mesh adapters, and `environment_lighting`) observes simulation and content from the rendering layer. FORM and particle simulation return plain data, sky settings belong to the renderer, and post-processing profiles are foundation data. These boundaries have no migration exception list.
