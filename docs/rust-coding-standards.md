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

## Layers and Crates
Each crate owns one dependency layer from `architecture/layers.json`: `joy-foundation`, `joy-simulation`, `joy-form` (content), `joy-render`, and `joy-platform`. A crate may depend only on lower layers. `joy-engine` is the facade that games depend on. Games, tools, and examples use `joy-engine` exports and never depend on `wgpu`, `winit`, `rapier3d`, or `egui` directly; this mirrors the TypeScript rule that no consumer imports luma.gl. `scripts/check-rust-layers.mjs` verifies the crate graph during `npm run check`.

Keep `cfg(target_arch = "wasm32")` branches inside `joy-platform` and the surface/adapter setup in `joy-render`. Simulation, content, and foundation code must build identically for native and WebAssembly targets.

## Ownership and Lifetimes
Rust ownership replaces most explicit `destroy()` calls: resources are released by `Drop`, in reverse field order. Order owner fields so dependents drop before the resources they borrow (for example, GPU pipelines before the device wrapper), and comment the order when it matters. Provide an explicit teardown method only when early release, error reporting, or idempotent shutdown is part of the contract.

Group struct fields by responsibility (owned resources, borrowed configuration, transient frame state) with short comments, as in the TypeScript renderer owners. Prefer handles and IDs (`EntityId`, `MeshInstanceId`) over long-lived references between owners. Shared systems support independent instances; avoid global mutable state, `static mut`, and lazily initialized singletons that hold gameplay state.

## Public Contracts
Every public item has a doc comment (`missing_docs` is a workspace lint). Describe purpose, parameters, observable behavior, return values, and failure. At simulation and render boundaries, state time units (`elapsed_seconds`, `timestamp_milliseconds`), coordinate spaces (Y-up world, meters, XYZ Euler radians applied X then Y then Z), mutation, and who owns returned data. Include units in names where ambiguity matters.

Validate authored or external data once at the public boundary and return typed errors (`thiserror` enums per crate). Never panic on authored content, user input, or GPU capability differences. Reserve `expect` for internal invariants, with a message explaining the invariant.

Use enums with `serde` renames for serialized vocabularies instead of repeated string literals; keep serialized spellings stable with the TypeScript engine so the same `.joylevel`, `.form`, and profile files load in both.

## Determinism
Keep gameplay randomness separate from cosmetic randomness. Use `joy_foundation::SeededRandom` (bit-identical to the TypeScript `createSeededRandom`) where reproducibility matters. Physics uses rapier's enhanced-determinism mode so native and WebAssembly runs agree. Preserve default values, time units, update order, random-number consumption, and collision semantics when porting behavior; separate intentional behavior changes from structural ports and document them.

## Shaders
Author GPU programs as standalone WGSL files under the owning crate's `shaders/` directory and load them with `include_str!`. WGSL is the only shading language; wgpu translates it for WebGL 2, Vulkan, Metal, and DirectX 12. Do not build shader source with string formatting beyond selecting whole files.

## Unsafe Code
`unsafe_code` is forbidden workspace-wide. Use `bytemuck` derives for GPU data.

## Tests
Follow the repository's [unit test scope](../AGENTS.md#unit-test-scope). Prefer behavior tests that protect a player, author, or engine consumer. When porting a TypeScript subsystem, add parity tests that compare against values recorded from the TypeScript reference (seeded random streams, environment frames, transforms, and FORM output). GPU tests must skip with a clear message when no adapter is available instead of failing.

## Review Checklist
- Does each crate depend only on lower layers, and do consumers use only `joy-engine`?
- Can a reader identify owners, borrowed collaborators, and plain records?
- Are units, coordinate spaces, ownership, and failure documented at public boundaries?
- Is authored data validated once, with typed errors rather than panics?
- Do native and WebAssembly builds share the same simulation and content code?
- Were `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` run?
