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
The engine is a workspace of system crates in `crates/` (`joy-core`, `joy-render-types`, `joy-terrain`, `joy-environment`, `joy-physics`, `joy-gameplay`, `joy-form`, `joy-fx`, `joy-dcc-gltf`, `joy-project`, `joy-renderer`, `joy-scene`, `joy-app`, `joy-input`, `joy-audio`, `joy-storage`, `joy-ui`, `joy-devtools`, and `joy-authoring`) behind the `joy-engine` facade, whose `src/` only re-exports them (`rendering`, `particles`, `terrain`, `runtime`, and `development` each gather several crates). Each crate names its layer in its `Cargo.toml` (`[package.metadata.joy] layer`): foundation, simulation, content, rendering, or platform. A crate may depend only on engine crates in its own layer or the layers below it, and on backend crates only from their owning layers (`[workspace.metadata.joy]` in the workspace manifest); `npm run check` enforces both. Refer to items through their owning crate and module path (`joy_physics::PhysicsWorld`, `crate::rendering::MeshAsset`), never through the facade's re-exports, which are the game-facing API in `src/lib.rs`. Each crate's root module is its `lib.rs`; its integration tests live in its own `tests/integration/main.rs` with fixtures in `tests/fixtures/`. Name Rust files in snake case (`physics_world.rs`; a module's unit tests add a `.test.rs` suffix, as in `physics_world.test.rs`), and give module folders a `mod.rs`.

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
Keep gameplay randomness separate from cosmetic randomness. Use `joy_engine::core::SeededRandom` where reproducibility matters. Simulation runs in `f64` (`math::DVec2`, `DVec3`), whose arithmetic, `sqrt`, and vector lengths are identical on every target; transcendental functions are not, so `f64` code calls `math::deterministic` (`sin`, `atan2`, `exp`, `powf`, and the rest, backed by the portable `libm` crate) instead of the standard methods. The workspace `clippy.toml` rejects the standard `f64` versions, and glam's own trigonometry uses `libm` through its `libm` feature. Engine crates and apps name math types through `joy_core::math` / `joy_engine::math`, never `glam` directly. Simulations tuned per fixed tick feed time to `core::FixedStepLoop`. Seed unrepeatable sessions from `runtime::unix_time_milliseconds`, and read capture/development switches through `runtime::launch_parameter` (browser query or native `--name=value`). Physics uses rapier's enhanced-determinism mode. Preserve defaults, time units, update order, random-number consumption, and collision semantics during refactors; document intentional changes.

## Shaders
Author GPU programs as standalone WGSL files in `crates/joy-renderer/src/shaders/` and load them with `include_str!`. Scene-effect programs live under `crates/joy-renderer/src/scene_effects/shaders/`. WGSL is the only shading language; wgpu translates it for WebGL 2, Vulkan, Metal, and DirectX 12. Do not build shader source with string formatting beyond selecting whole files.

## Unsafe Code
`unsafe_code` is forbidden workspace-wide. Use `bytemuck` derives for GPU data.

## Tests
Every package (engine crates, the facade, Joy Editor, the tools, and games) keeps test code out of the files it tests:

- Unit tests that need private items live beside their module as `<file>.test.rs` (`src/scene/mesh.rs` → `src/scene/mesh.test.rs`; `src/scene/mod.rs` → `src/scene/mod.test.rs`), so a module and its tests sort together and move together. The module declares them last, as `#[cfg(test)] #[path = "mesh.test.rs"] mod tests;` (the `#[path]` always names a file in the same folder), and the test file reaches the module's items through `use super::*;`. Binaries in src/bin/ have no .test.rs; test them from tests/integration/ by running them (env!("CARGO_BIN_EXE_<name>")), and move logic that needs unit tests into the library. Prefer an integration test when the public API can express the behavior.
- `tests/integration/main.rs` and its modules: the package's integration tests, built as one binary that Cargo discovers on its own, so manifests list no test targets.
- `tests/fixtures/`: golden recordings and sample documents, read with `include_str!("../fixtures/…")`, from `CARGO_MANIFEST_DIR`, or (for golden recordings) with `golden_file!`.
- `tests/tooling/`: Node or Python tests for the package's own scripts.

Follow the repository's [unit test scope](../AGENTS.md#unit-test-scope). Prefer behavior tests that protect a player, author, or engine consumer. Preserve useful golden recordings for seeded randomness, environment frames, transforms, FORM output, and other compatibility contracts. GPU tests must skip with a clear message when no adapter is available instead of failing.

### Golden Recordings
A golden recording is a JSON fixture holding the inputs a test replays and the outputs the code produced when it was last recorded on purpose. `joy-test-support` (a dev-dependency for engine crates; `joy_engine::testing` for games, through the facade's `test-support` feature in their `[dev-dependencies]`) checks them:

- Open the fixture with `golden_file!("name.json")`, rebuild the recording from the current code (usually by cloning `recording()` and replacing each case's outputs with records in the same schema), and check it with `assert_matches`, or `assert_section_matches` when tests share one file.
- Numbers agree within a `Tolerance` relative to the larger of one and the recorded magnitude; strings, booleans, array lengths, and object keys must match exactly, so a record that stops producing a field, or starts producing a new one, fails until it is re-recorded.
- When behavior changes on purpose, re-record with `JOY_UPDATE_FIXTURES=1 cargo test -p <package>` and review the fixture diff with the code change. Re-recording keeps every number that still agrees within the tolerance, the file's layout, and its number spelling, so the diff shows only what changed. Never re-record to make an unexplained difference go away.

## Review Checklist
- Does each module name only its own and lower layers, and do consumers use only `joy_engine` exports?
- Can a reader identify owners, borrowed collaborators, and plain records?
- Are units, coordinate spaces, ownership, and failure documented at public boundaries?
- Is authored data validated once, with typed errors rather than panics?
- Do native and WebAssembly builds share the same simulation and content code?
- Were `cargo +nightly fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` run?
