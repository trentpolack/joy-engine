# Rust Coding Standards
Joy Engine follows this reading model: a small procedural composition root, stateful owners with documented lifetimes, and plain records for data. A reader should be able to follow startup, one frame, failure, and teardown without reconstructing hidden state. This guide applies [the shared coding standards](coding-standards.md) and JoyCore's organization to Rust and WGSL.

## Formatting
The vertical import layout requires nightly rustfmt. Install it with `rustup toolchain install nightly --profile minimal --component rustfmt`. Formatting commands select nightly explicitly; Clippy, tests, and builds use the stable toolchain.

`rustfmt` (configured by `rustfmt.toml`, matching joy-apps) is authoritative for layout. The retained Node tooling spacing conventions do not apply to Rust. Run `cargo +nightly fmt --all` before validation; CI-equivalent checks use `cargo +nightly fmt --all -- --check`.

The intent rules still apply:
- Put every multi-step branch in braces and keep one meaningful action per line.
- Parenthesize mixed arithmetic when it clarifies the order of operations, even when it is functionally redundant; clippy's `precedence` lint is allowed for this reason.
- Use decimals only for floating-point values (`2.0` for a float multiplier, `2` for an index or count).
- Separate logical chunks of code with blank lines and short section comments, in the JoyCore style.

## File Layout
Every authored source file starts with the two-line copyright and license header (`//` for Rust and WGSL, `#` for TOML):

```rust
// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
```

Then: module documentation (`//!`), `use` declarations, named constants and defaults, public records and enums, the primary owner with its constructor, public operations, and teardown, and finally private helpers. Keep the public control path near the top of a module and narrow helpers below it. Keep one primary responsibility per module.

## Layers and Modules
The engine is a workspace of system crates in `crates/` (`joy-core`, `joy-environment`, `joy-physics`, `joy-gameplay`, `joy-form`, `joy-fx`, `joy-dcc-gltf`, `joy-project`, `joy-renderer`, `joy-scene`, `joy-app`, `joy-input`, `joy-audio`, `joy-storage`, `joy-ui`, `joy-devtools`, and `joy-authoring`) behind the `joy-engine` facade, whose `src/` only re-exports them (`rendering`, `particles`, `runtime`, and `development` each gather several crates). `architecture/layers.json` assigns every crate and facade module a layer: foundation, simulation, content, rendering, or platform. A crate may depend only on engine crates in its own layer or the layers below it, and on backend crates only from their owning layers; a facade module follows the same rule for the `crate::<module>` paths and engine crates it names. `npm run check` enforces both. Refer to items through their owning crate and module path (`joy_physics::PhysicsWorld`, `crate::rendering::MeshAsset`), never through the facade's re-exports, which are the game-facing API in `src/lib.rs`. Each crate's root module is its `lib.rs`; its integration tests live in its own `test/rust/main.rs` with fixtures in `test/fixtures/`. Name Rust files in snake case (`physics_world.rs`), and give module folders a `mod.rs`.

Games, tools, and examples use `joy_engine` exports and never depend on the engine's system crates, `wgpu`, `winit`, `rapier3d`, or `egui` directly.

Keep `cfg(target_arch = "wasm32")` branches inside the platform crates' browser backends (their `web` modules, such as `web.rs` and `web_gamepad.rs`) and the surface/adapter setup in `joy-renderer`'s `gpu` module. Simulation, content, and foundation code must build identically for native and WebAssembly targets.

## Ownership and Lifetimes
Rust ownership replaces most explicit `destroy()` calls: resources are released by `Drop`; struct fields drop in declaration order. Order owner fields so dependents drop before the resources they borrow (for example, GPU pipelines before the device wrapper), and comment the order when it matters. Provide an explicit teardown method only when early release, error reporting, or idempotent shutdown is part of the contract.

Group struct fields by responsibility (owned resources, borrowed configuration, transient frame state) with short comments. Prefer handles and IDs (`EntityId`, `MeshInstanceId`) over long-lived references between owners. Shared systems support independent instances; avoid global mutable state, `static mut`, and lazily initialized singletons that hold gameplay state.

## Public Contracts
Every public item has a doc comment (`missing_docs` is a workspace lint). Describe purpose, parameters, observable behavior, return values, and failure. At simulation and render boundaries, state time units (`elapsed_seconds`, `timestamp_milliseconds`), coordinate spaces (Y-up world, meters, XYZ Euler radians applied X then Y then Z), mutation, and who owns returned data. Include units in names where ambiguity matters.

Validate authored or external data once at the public boundary and return typed errors (`thiserror` enums per module). Never panic on authored content, user input, or GPU capability differences. Reserve `expect` for internal invariants, with a message explaining the invariant.

Use enums with `serde` renames for serialized vocabularies instead of repeated string literals; keep serialized spellings stable so existing `.joylevel`, `.form`, and profile files remain compatible.

## Determinism
Keep gameplay randomness separate from cosmetic randomness. Use `joy_engine::core::SeededRandom` where reproducibility matters. Portable helpers in `core::js_math` and `core::js_hypot` preserve the numerical semantics used by recorded regression fixtures across native and WebAssembly targets. Games tuned per fixed tick feed time to `runtime::FixedStepLoop`. Seed unrepeatable sessions from `runtime::unix_time_milliseconds`, and read capture/development switches through `runtime::launch_parameter` (browser query or native `--name=value`). Physics uses rapier's enhanced-determinism mode. Preserve defaults, time units, update order, random-number consumption, and collision semantics during refactors; document intentional changes.

## Shaders
Author GPU programs as standalone WGSL files in `crates/joy-renderer/src/shaders/` and load them with `include_str!`. Scene-effect programs live under `crates/joy-renderer/src/scene_effects/shaders/`. WGSL is the only shading language; wgpu translates it for WebGL 2, Vulkan, Metal, and DirectX 12. Do not build shader source with string formatting beyond selecting whole files.

## Unsafe Code
`unsafe_code` is forbidden workspace-wide. Use `bytemuck` derives for GPU data.

## Tests
Follow the repository's [unit test scope](../AGENTS.md#unit-test-scope). Prefer behavior tests that protect a player, author, or engine consumer. Preserve useful recorded-reference tests for seeded randomness, environment frames, transforms, FORM output, and other compatibility contracts. GPU tests must skip with a clear message when no adapter is available instead of failing.

## Review Checklist
- Does each module name only its own and lower layers, and do consumers use only `joy_engine` exports?
- Can a reader identify owners, borrowed collaborators, and plain records?
- Are units, coordinate spaces, ownership, and failure documented at public boundaries?
- Is authored data validated once, with typed errors rather than panics?
- Do native and WebAssembly builds share the same simulation and content code?
- Were `cargo +nightly fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` run?
