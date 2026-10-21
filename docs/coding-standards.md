# Coding Standards
Use a small procedural composition root, stateful owners with documented lifetimes, and plain records for data. A reader should follow startup, one frame, failure, and teardown without reconstructing hidden state. Keep one primary responsibility per module and prefer named intermediate data over dense expressions.

Rust is the runtime language. Follow [Rust Coding Standards](rust-coding-standards.md) for modules, ownership, public contracts, determinism, shaders, and formatting. `rustfmt` is authoritative for Rust layout. JoyCore remains the reference for readable organization, intent comments, and separation of configuration from transient state; adapt those principles to Rust ownership rather than copying another engine's architecture.

## Public Contracts and Ownership
Document units, coordinate spaces, mutation, ownership, and failure at public boundaries. Validate authored data once and return typed errors. Keep resource acquisition and release together; independent sessions must not share mutable global gameplay state. Use IDs and explicit collaborators where cross-owner references would obscure lifetimes.

Keep the main execution path near the top of a module and narrow helpers below. Comments explain constraints, decisions, and non-obvious tradeoffs. Do not narrate syntax, compress source, or leave commented-out code as a substitute for cleanup.

## Configuration and Behavior
Games own tuning and palettes in Rust configuration modules. External documents require an explicit validated serialization boundary. Use enums or scoped constants for meaningful state and vocabulary, preserving serialized spellings. Engine defaults belong beside the mechanism they configure.

Preserve time units, update order, random-number consumption, collision behavior, and rendering order during structural refactors. Keep intentional behavior changes separate. Gameplay randomness stays independent from cosmetic randomness; deterministic scenarios use seeded streams and portable numerical helpers.

## Node and Extension Tooling
JavaScript remains appropriate for build orchestration, captures, deployment, patch delivery, and browser infrastructure. The VS Code extension may use TypeScript for its host API. These tools must not become a second game or engine implementation.

Use readable native modules, descriptive names, explicit state, small functions, and documented JSDoc contracts where useful. Keep two-space formatting and the existing tooling conventions: braces around every branch (never a single-line `if`), `if(`/`for(`/`while(` spacing, no unnecessary trailing commas, compound assignment without a space before the operator (`x+= 1`), no spaces around `*`, `/`, or exponents while `+` and `-` keep them, readable grouped arithmetic, and JSDoc function descriptions starting on the line after the opening `/**`. Classes suit resource-owning state; ordinary data should remain plain records. Declare cleanup and callback ownership explicitly, including subprocesses, servers, file handles, and listeners.

Keep configuration and mutable state distinct. Validate filesystem/network boundaries, preserve existing paths and serialized values, and avoid dense callback chains with hidden side effects. Maintain the npm workspace metadata and root lockfile when dependencies change.

## Review Checklist
- Can a reader identify owners, collaborators, data, and the main control path?
- Are units, coordinates, mutation, and failure documented?
- Are authored inputs validated before mutating accepted state?
- Do native and browser builds share simulation and content code?
- Did checks and the affected builds run, with captures for visual changes?
