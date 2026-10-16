# Dependency Layers
Joy Engine uses a one-way dependency model so simulation runs without a window, canvas, or GPU:

1. Foundation contains shared data and algorithms.
2. Simulation contains environment, entity, gameplay, and physics state.
3. Content contains asset parsing, compilation, and script information.
4. Rendering contains GPU presentation and effects.
5. Platform contains runtime, browser, and development integrations.

`architecture/layers.json` records the allowed edges. `npm run check` runs `scripts/check-boundaries.mjs`, which checks Rust `crate::<module>` paths, grouped imports, and documentation paths against the owning top-level source folder. Internal modules use module-qualified paths; the re-exports in `src/lib.rs` are the consumer-facing API.

Backend crates also have owners: wgpu belongs to rendering and platform, rapier to simulation, and winit and egui to platform. Games, tools, and examples depend on `joy-engine`, using its re-exported UI contracts rather than backend dependencies. The engine never depends on those consumers.

`World` is the headless composition boundary for entities and physics. `LevelScene` and `AssetScene` observe simulation from rendering. FORM returns plain geometry, and post-processing profiles are foundation data. These boundaries have no migration exception list.
