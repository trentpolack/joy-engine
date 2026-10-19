# Dependency Layers
Joy Engine uses a one-way dependency model so simulation runs without a window, canvas, or GPU:

1. Foundation contains shared data and algorithms.
2. Simulation contains environment, entity, gameplay, and physics state.
3. Content contains asset parsing, compilation, and script information.
4. Rendering contains GPU presentation and effects.
5. Platform contains the application shell, input, audio, storage, UI, development tools, and the authoring kit, with their browser backends.

The policy lives in the Cargo manifests. Each engine crate names its layer in `[package.metadata.joy]` (`layer = "simulation"`; `joy-test-support` marks itself `test-support = true`), and the workspace manifest's `[workspace.metadata.joy]` orders the layers and lists the backend crates' owning layers. `npm run check` runs `scripts/check-boundaries.mjs`, which reads both through `cargo metadata`: an engine crate may depend only on crates in its own or an earlier layer, on backend crates only from their owning layers, and on test support only as a dev-dependency; Joy Editor and the labs may depend on `joy-engine` but on no engine or backend crate; and the facade's examples, which share its dependencies, may not name engine or backend crates in their source. Cargo itself keeps a crate from naming anything it does not declare, so the manifests are the whole boundary. Internal code uses crate- and module-qualified paths; the re-exports in `src/lib.rs` are the consumer-facing API.

Backend crates also have owners: wgpu belongs to rendering and platform, rapier to simulation, and winit and egui to platform. Games, tools, and examples depend on `joy-engine`, using its re-exported crates and UI contracts rather than system crates or backend dependencies. The engine never depends on those consumers.

`World` is the headless composition boundary for entities and physics. The renderer depends only on foundation data and particle documents; `joy-scene` (`LevelScene`, `AssetScene`, the FORM mesh adapters, and `environment_lighting`) observes simulation and content from the rendering layer. FORM and particle simulation return plain data, and sky and post-processing settings belong to the renderer. Gameplay owns the component vocabulary and the simulation components; `joy-scene` owns the presentation components (`render`, `light`, `effect`, and `postProcess`) and resolves post-process volumes, so simulation never depends on rendering. These boundaries have no migration exception list.
