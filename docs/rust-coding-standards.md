# Rust Coding Standards
The Rust engine follows the same reading model as the TypeScript engine: a small procedural composition root, stateful owners with documented lifetimes, and plain records for data. A reader should be able to follow startup, one frame, failure, and teardown without reconstructing hidden state. This guide adapts [the TypeScript and JavaScript standards](coding-standards.md) and JoyCore's organization to Rust; where the two disagree, the Rust-specific rule below wins for `.rs` and `.wgsl` sources.

## Formatting
`rustfmt` (configured by `rustfmt.toml`, matching joy-apps) is authoritative for layout. The JavaScript spacing rules (`if(`, `x+= 1`, operator spacing, and trailing commas) describe a style rustfmt cannot express, so they do not apply to Rust. Run `cargo fmt --all` before validation; CI-equivalent checks use `cargo fmt --all -- --check`.

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
The engine is one crate, `joy-engine`, whose modules live in the TypeScript `src/` folders and inherit their layers from `architecture/layers.json`: `core` (foundation); `environment`, `physics`, and `gameplay` (simulation); `form` (content); `rendering` (rendering); and `runtime`, `development`, and `browser` (platform). A module may name (`crate::<module>`) only its own layer and the layers below it, and backend crates only from their owning layers; `npm run check` enforces both. Refer to items through their module path (`crate::rendering::MeshAsset`), never through the crate root's re-exports, which are the game-facing API in `src/lib.rs`. Name Rust files after the TypeScript files they replace, in snake case (`physics-world.ts` → `physics_world.rs`), and give each folder a `mod.rs`.

Games, tools, and examples use `joy_engine` exports and never depend on `wgpu`, `winit`, `rapier3d`, or `egui` directly; this mirrors the TypeScript rule that no consumer imports luma.gl.

Keep `cfg(target_arch = "wasm32")` branches inside the platform modules and the surface/adapter setup in `rendering::gpu`. Simulation, content, and foundation code must build identically for native and WebAssembly targets.

## Ownership and Lifetimes
Rust ownership replaces most explicit `destroy()` calls: resources are released by `Drop`, in reverse field order. Order owner fields so dependents drop before the resources they borrow (for example, GPU pipelines before the device wrapper), and comment the order when it matters. Provide an explicit teardown method only when early release, error reporting, or idempotent shutdown is part of the contract.

Group struct fields by responsibility (owned resources, borrowed configuration, transient frame state) with short comments, as in the TypeScript renderer owners. Prefer handles and IDs (`EntityId`, `MeshInstanceId`) over long-lived references between owners. Shared systems support independent instances; avoid global mutable state, `static mut`, and lazily initialized singletons that hold gameplay state.

## Public Contracts
Every public item has a doc comment (`missing_docs` is a workspace lint). Describe purpose, parameters, observable behavior, return values, and failure. At simulation and render boundaries, state time units (`elapsed_seconds`, `timestamp_milliseconds`), coordinate spaces (Y-up world, meters, XYZ Euler radians applied X then Y then Z), mutation, and who owns returned data. Include units in names where ambiguity matters.

Validate authored or external data once at the public boundary and return typed errors (`thiserror` enums per module). Never panic on authored content, user input, or GPU capability differences. Reserve `expect` for internal invariants, with a message explaining the invariant.

Use enums with `serde` renames for serialized vocabularies instead of repeated string literals; keep serialized spellings stable with the TypeScript engine so the same `.joylevel`, `.form`, and profile files load in both.

## Determinism
Keep gameplay randomness separate from cosmetic randomness. Use `joy_engine::core::SeededRandom` (bit-identical to the TypeScript `createSeededRandom`) where reproducibility matters. Simulations ported from TypeScript use `core::js_math` for `Math.sin`, `Math.cos`, `Math.atan2`, `Math.exp`, `Math.log`, `Math.pow`, and `Math.round` (portable `libm` results, identical natively and in WebAssembly) and `core::js_hypot` for `Math.hypot`; games tuned per fixed tick feed frame time to `runtime::FixedStepLoop` (the TypeScript `startGameLoop`). Seed unrepeatable sessions from `runtime::unix_time_milliseconds` (`Date.now()`), and read development switches such as capture scenarios with `runtime::launch_parameter` (URL query in browsers, `--name=value` natively). Physics uses rapier's enhanced-determinism mode so native and WebAssembly runs agree. Preserve default values, time units, update order, random-number consumption, and collision semantics when porting behavior; separate intentional behavior changes from structural ports and document them.

## Shaders
Author GPU programs as standalone WGSL files in `src/rendering/shaders/` and load them with `include_str!`. The top-level `shaders/` folder belongs to the TypeScript renderer. WGSL is the only shading language; wgpu translates it for WebGL 2, Vulkan, Metal, and DirectX 12. Do not build shader source with string formatting beyond selecting whole files.

## Unsafe Code
`unsafe_code` is forbidden workspace-wide. Use `bytemuck` derives for GPU data.

## Tests
Follow the repository's [unit test scope](../AGENTS.md#unit-test-scope). Prefer behavior tests that protect a player, author, or engine consumer. When porting a TypeScript subsystem, add parity tests that compare against values recorded from the TypeScript reference (seeded random streams, environment frames, transforms, and FORM output). GPU tests must skip with a clear message when no adapter is available instead of failing.

## Review Checklist
- Does each module name only its own and lower layers, and do consumers use only `joy_engine` exports?
- Can a reader identify owners, borrowed collaborators, and plain records?
- Are units, coordinate spaces, ownership, and failure documented at public boundaries?
- Is authored data validated once, with typed errors rather than panics?
- Do native and WebAssembly builds share the same simulation and content code?
- Were `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` run?
