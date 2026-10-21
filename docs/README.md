# Joy Engine Documentation
Joy Engine is a Rust workspace shared by native games, WebAssembly games, and authoring tools: system crates in `crates/` behind the `joy-engine` facade. The public API is [src/lib.rs](../src/lib.rs). Generate the API reference with `npm run docs:api` or browse it with `cargo doc --no-deps --open` from the engine root.

## Guides
- [Runtime and browser builds](rust.md): setup, frame phases, renderer, and platform behavior.
- [Architecture](overview.md) and [dependency layers](architecture/dependency-layers.md).
- [Coding standards](coding-standards.md) and [Rust conventions](rust-coding-standards.md).
- [Scenes](scenes.md), [meshes and materials](materials.md), [lighting and shadows](lighting.md), [scene effects](scene-effects.md), [postprocessing](postprocessing.md), and [shaders](shaders.md).
- [Gameplay](gameplay.md), [simulation and physics](rust-simulation.md), and [environment](environment.md).
- [FORM](rust-form.md), [particles](particles.md), [terrain](terrain.md), and [content formats](rust-content.md).
- [Input](input.md), [audio](audio.md), and [storage](rust-storage.md).
- [Live tuning](tuning.md), [asset hot reload](hot-reload.md), the [console and profiling](console-and-profiling.md), and the [authoring kit](rust-authoring.md).
- [Joy Editor](../editor/README.md) and [engine examples](../examples/README.md).
- [Patch delivery](patch-delivery.md).
- [Repository audit](repository-audit.md): repeatable maintenance checks and independent audit tests.

## Ownership
Games own rules, tuning, assets, cameras, and effect composition. The engine owns reusable mechanisms and never imports a game or tool. Resources have explicit Rust owners; renderers borrow presentation state and never advance simulation or consume gameplay randomness. Separate document state from running simulation and from editor-only preview settings.

Use `npm run check`, `npm test`, and an affected native/browser build when changing the engine. Rendering changes also need captures on the affected backends; passing headless or software-rendered tests does not establish hardware performance or browser compatibility.
