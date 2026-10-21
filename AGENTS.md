# AGENTS.md
This file applies to the entire repository unless a package-specific `AGENTS.md` says otherwise for that package.

## Overview
Joy Engine is a Rust game engine for native applications and browser WebAssembly (WebGPU with WebGL 2 fallback) for prototyping, procedural generation and stylized games. This repository holds the engine package, Joy Editor and its FORM EDITOR and JOYFX EDITOR tools, and independently runnable engine examples.

Prefer readable control flow, named data, and small modules over clever abstractions or compression. If a change makes code harder to scan, explain why the tradeoff is worthwhile. There are additional specific cases in the "Code Standards and AI Agent Rules" section of this doc.

## Setup and validation
When working in this repository on its own, run commands from its root. When it is included in a larger workspace, follow that workspace's workflow and use its existing commands to edit and validate changes across directories:
```bash
npm ci
npm run check
npm test
```

`npm run check` and `npm test` validate the Rust engine/workspace and retained Node tooling. Run `cargo +nightly fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo build --target wasm32-unknown-unknown --lib --examples` as appropriate; see [the runtime guide](docs/rust.md). Run `npm run docs:api` when public API documentation changes and review the generated rustdoc; do not commit generated `target/doc/`.

Before completing a change:
1. Run the narrowest relevant check while iterating.
2. Run the repository checks and tests above; they already cover the engine Cargo workspace.
3. Build the affected workspace (`npm run build --workspace <name>`).
4. For rendering, gameplay-facing or tool UI changes, also run the available hosting workspace's integration validation and browser captures. Record browser limitations and capture failures explicitly.

Joy Editor runs natively for project filesystem authoring; its browser build uses bundled examples. See [the editor README](editor/README.md).

Do not commit `node_modules`, generated `dist` folders, logs, caches, or local environment files (i.e., follow the `.gitignore` rules).

## Assets and Intellectual Property/Trademarks
- Treat external games as references for principles and feel, never as sources for copied code, art, audio, layouts, or exact effects.
- When ownership or redistribution rights are unclear, stop, and ask.

## Change Discipline
Read the root README and the affected package's README before making structural changes. Preserve unrelated user work and avoid broad rewrites outside the requested package.

When this repository is included as a submodule in a larger workspace, treat it as a directory for the requested edits and validation. Do not introduce a separate repository handoff or require Git state changes to complete the task. Leave submodule commits, branches, pushes, merges, tags, and the parent pin to the user unless explicitly requested; this applies to local, IDE, and cloud work.

Keep the engine's public API small and document why new shared behavior belongs in the engine. For editor- or tool-only changes, avoid touching the engine package or root tooling unless the change cannot be supported cleanly otherwise.

In addition:
- Update the root-level `CHANGELOG.md` whenever a major/minor/patch change is made; don't get too granular, just calling out major changes.
- Group changelog entries under one `## MM-DD-YYYY` heading per day, newest first, with `###` headings for projects or topics. Reuse the day's existing heading rather than creating duplicate date sections. Use the completion date in `America/Detroit` for current work and the introducing commit's date in that timezone for historical backfills. Do not add `Unreleased` sections or replace known historical dates with today's date. Automated changelog updates must preserve existing entries and this daily structure.
- When encountering new/found work, bugs, potential future work, deprecated logic/packages, adding tech debt then create an Issue in GitHub. Agents whose instructions forbid filing issues (see `CLAUDE.md`) list these as proposed issues in their completion report instead.
  - Don't create "checklist" issues; just file actionable issues.
  - For follow-up/future work, create a GitHub Issue; do not create standalone markdown files like `follow-ups.md`.
  - Do not file issues for work that is being addressed within current task(s); file issues for work that needs eyes on it for future work.
  - Apply appropriate labels/tags to issues.
- When engine work absorbs behavior that consumers implement themselves (games in a hosting workspace, Joy Editor, FORM EDITOR, JOYFX EDITOR, or examples), migrate those consumers in the same change and delete their now-redundant code. A new engine mechanism without its consumer migration is unfinished work. Name what each consumer removed in the completion report.

### Completion Reports
Keep the whole summary under roughly 300 words and link out instead of expanding. State what changed, what was validated (and what could not be), consumer code removed, and proposed follow-up issues. Use the completion date in `America/Detroit`.

## Joy Engine Cloud Patch Delivery
- For engine file changes delivered as a cloud bundle, produce `.patches/joy-engine.patch`, `.patches/joy-engine-base.txt`, `.patches/joy-engine-manifest.txt`, and `.patches/joy-engine-tests.txt` in this repository's root. A hosting workspace may specify its own delivery directory; follow that workspace's instructions when the engine is nested there.
- Record the engine commit SHA from the start of the task. Include the task's modified/deleted files, new authored files, and binary changes relative to that revision, including task changes already committed in the cloud. Patch paths and the manifest must be engine-relative, with one changed path per manifest line. Include validation results in the test report.
- Exclude dependencies, generated build output, secrets, `.patches/`, and submodule pointer changes. Verify the patch against a disposable copy of the recorded base without changing the real repository's index or refs.
- Keep `.patches/` ignored. If the cloud task diff is the transport, force-add only these four delivery files in that cloud checkout; this does not authorize staging them locally or merging transport files into source history. Return all four files through the actual task diff or downloadable artifacts, and verify that they are present. A report without its patch is incomplete delivery; report missing delivery as a blocker.
- Locally, run `npm run fetch-joy-engine-patch -- <task-id>` and `npm run apply-joy-engine-patch` from this repository's root. Fetch verifies and saves the ignored bundle without importing source changes; apply preserves staging and refs, reconciles in a disposable repository, and removes all four delivery files only after success or already-applied detection. Conflicts retain the bundle. Never run `git apply --3way` in the real working checkout.
- See [Patch Delivery](docs/patch-delivery.md) for saved task diffs, noninteractive application, and hosting-workspace behavior.

## Unit Test Scope
Keep the unit suite focused on core behavior and costly regressions: document and asset round trips, bounded or deterministic simulation, resource ownership and cleanup, backend contracts and fallback, and gameplay state transitions. Extend an existing scenario when it already covers the same risk. Add a new test only when it protects a distinct failure that matters to a player, author, or shared-engine consumer.

Do not add tests solely for coverage counts, trivial helpers, fixed tuning values, visual styling, source-text patterns, or implementation details already exercised through behavior. Review appearance and game feel with the existing browser checks and captures. Preserve regression cases for data loss, resource leaks, runaway work, and compatibility failures.

## Workspace structure
- `crates/`: the engine's system crates (`joy-core`, `joy-render-types`, `joy-environment`, `joy-physics`, `joy-gameplay`, `joy-form`, `joy-fx`, `joy-dcc-gltf`, `joy-project`, `joy-renderer`, `joy-scene`, `joy-app`, `joy-input`, `joy-audio`, `joy-storage`, `joy-ui`, `joy-devtools`, and `joy-authoring`), each with its own tests and fixtures; each crate names its architecture layer in its `Cargo.toml` (`[package.metadata.joy]`).
- `src/`, `schemas/`, `assets/`: the `joy-engine` facade package (public entry points are listed in [README.md](README.md)), which re-exports the system crates under their module names, plus shared schemas and assets.
- `tests/`, `scripts/`, `docs/`: the facade's tests and the tooling tests, tooling, guides and API documentation sources. Every package keeps unit tests beside the module they test (`<file>.test.rs`) and its other tests in its own `tests/` folder (`integration/`, `fixtures/`, and `tooling/`); see the [Rust coding standards](docs/rust-coding-standards.md#tests).
- `editor/`: Joy Editor (`joy-editor`), a separate Rust package with npm build metadata.
- `tools/`: FORM EDITOR (`form-editor`) and JOYFX EDITOR (`joyfx-editor`), standalone tool packages that Joy Editor hosts as tabs.
- `Cargo.toml`, `src/**/*.rs`, `tests/integration/`: the engine workspace, the `joy-engine` facade crate, and its integration tests. `joy-renderer` is the only renderer; tools and FORM content draw through it (by way of `joy-scene`) rather than owning renderers.
- `examples/*`: engine feature demonstrations. A hosting workspace can publish examples that opt in with `gameSite.category: "example"`; examples remain independent of games and the host.

The engine package never imports the editor, tools or examples. The editor depends on the engine crate and hosts the tool editor crates. Tests, fixtures and tooling live inside the owning package. Nothing in this repository may depend on a particular game; game-specific fixtures belong in the consuming repository.

Move code into the engine (the engine crate that owns it) when it is stable behavior that more than one game, tool, or example needs.

## Code Standards and AI Agent Rules
- Follow [**Coding Standards**](docs/coding-standards.md) and [**Rust Coding Standards**](docs/rust-coding-standards.md). Apply them to changed code as well as new code.
- Always use [JoyCore](https://github.com/trentpolack/JoyCore) as a reference for code organization, commenting style, naming, clarity, and overall readability. Consult relevant examples when implementing or refactoring code, and adapt their principles to this repository's Rust architecture, even when JoyCore uses a different language or engine.
- Write runtime code in Rust with descriptive names and documented public contracts. Keep browser-specific glue in the platform crates' `web` modules.
- Keep shared numeric and data contracts in `joy-core` (`crates/joy-core/`) and GPU-free render data (meshes, materials, and lights) in `joy-render-types` (`crates/joy-render-types/`), with consumer exports in `src/lib.rs`. Games own their presentation and gameplay contracts.
- New games are Cargo workspace members that depend on `joy-engine`; retain npm metadata and scripts for site orchestration. Node tooling and the VS Code extension may use their appropriate host languages.
- Games read authored text assets (`.form`, `.forminst`, `.joyfx`, `.joylevel`, `.joyobject`, tuning JSON) through `GameAssets` (`joy_engine::game_assets!` or `joy_engine::source_root!`) rather than bare `include_str!`, so native development builds hot reload edits while browser and release builds stay embedded. Build levels from authored `.joylevel` documents with `AssetScene::prepare_level` so FORM assets and their object placements reach play the same way they look in Joy Editor.
- Keep all common/engine functionality in the engine crates and consume it through `joy-engine`; do not add parallel implementations or compatibility packages.
- Keep one primary responsibility per module.
- Put tuning values and palettes in the owning workspace's configuration modules.
- Comment intent, context, alternatives (if relevant), constraints, and non-obvious tradeoffs; do not narrate syntax.
- Prefer small functions, explicit state, and data with named fields.
- Do not commit minified, compressed, or generated source as authored code.
- Avoid new dependencies unless they materially simplify the work.
- Preserve npm workspaces and the root lockfile.
- Treat warnings and unsupported-browser states as part of the user experience.
- Keep keyboard, pointer, touch, and responsive behavior intact when changing gameplay or rendering.
- Don't use manual line breaks in markdown files; they're unnecessary.
- Don't add a line break in between markdown headers and content.
- Games, the editor, tools, and examples must consume engine functionality through `joy_engine` exports. Do not copy engine modules or depend directly on wgpu, winit, rapier, or egui; use engine-owned APIs and re-exports.
- Before changing an exported engine contract, locate all consumers, including Rust types, tests, fixtures, tooling, and documentation. Update affected consumers in the same change. Prefer a coordinated migration over permanent compatibility layers.
- Preserve existing gameplay behavior during engine extraction and refactoring. Preserve default values, time units, update order, random-number consumption, collision semantics, and rendering order unless the task explicitly changes them. Separate behavior changes from structural changes.
- Consumers pin a commit or release tag of this repository. When explicitly asked to tag a release, use `vX.Y.Z` from the engine package version. Keep the release branch releasable, and call out breaking changes to exported contracts in the CHANGELOG. Do not create private engine copies to freeze a consumer.
- Document time units, coordinate spaces, ownership, mutation, and resource lifecycle at shared API boundaries. Make seconds versus milliseconds and per-tick versus per-second values explicit.
- Shared systems must support independent instances and explicit cleanup. Avoid mutable module-level gameplay state. Owners must release event listeners, timers, audio resources, and GPU resources on disposal.
- Keep gameplay randomness separate from cosmetic randomness. Support seeded or injected randomness where reproducible simulation is needed so visual changes do not alter gameplay outcomes.
- Always aim to build out the core engine (`crates/`) with functionality that can benefit all current and future projects.
- Keep a consistent DPI/resolution for projects; ensure the 3D scene and HUD/UI elements have the proper scale for standard or high DPI displays. A higher resolution/DPI should not materially change how much the player can see.
- Aim for quality rendering; stylized does not have to mean compromised fidelity or effects.
- Games and previews render through Joy Engine. Use the shared egui HUD and development interfaces where appropriate; keep web page infrastructure separate from simulation and rendering.
- Browser rendering targets WebGPU with a WebGL 2 fallback. Native rendering uses the engine's wgpu backend selection.
- Favor explicit ownership, small modules, and clear state transitions. Use object-oriented patterns when they improve clarity, but do not force them where a simpler functional or data-driven shape is clearer.
- Keep code clear, well-formatted, and documented (in-line/commented).
- Organize retained Node tooling for a C/C++-friendly reading model: make ownership and lifetimes explicit, keep composition roots procedural, use classes for stateful owners, and treat plain records as structs. Prefer early returns, descriptive intermediate values, and named functions over dense callback chains or clever expressions.
- Keep public methods and the main control path near the top of a module; move narrow implementation helpers below them. Group related state and avoid hidden mutation, implicit singleton state, and abbreviations that make call sites harder to scan.
- Keep authored GPU programs in standalone `.wgsl` files, loaded with `include_str!`. Do not embed substantial shader implementations in formatted strings.
- Development entry points should expose useful startup, backend, lifecycle, and failure diagnostics without producing routine logs in production builds. Reusable frame/runtime inspection belongs in Joy Engine and should remain removable from production sessions.
- Verify development behavior through the actual npm entry point. The root `dev` command must serve development web builds; project `dev` commands run native development builds; production preview is explicitly named `preview`. Test production silence separately from development diagnostics.
- Test for Safari, Chrome, and Edge.
- Retained Node tooling follows the spacing and comment rules in the [coding standards](docs/coding-standards.md#node-and-extension-tooling); `rustfmt` controls Rust formatting.
- When using scalars, don't use decimals when the expected operation is essentially between integers. Use decimals when it's assumed that they're floating-point values.
- For clarity when multiple operations are happening, I like to make the order of operations clear with parentheses that may be _functionally_ pointless but improve readability.
- Games built on the engine need the standard CONFIG button/menu and JOY-EDITOR config button/menu; the engine owns those shared development widgets.
- Always avoid hard-coded strings that have gameplay/logic/engine relevance; prefer to create a const variable for the string and use _that_ elsewhere.
- Comment code and embrace the comfort of some extra line breaks when it comes to separating "chunks" of code; check out my general style of commenting/spacing in my JoyCore project: https://github.com/trentpolack/JoyCore/tree/develop/Source/JoyCore/Private/Systems; adapt the same readable organization to Rust, with `rustfmt` controlling layout.
- The header copyright/MIT license text for each file being created should be the copyright on one line and the license on the next line (except on form/joyfx scripts); i.e.:
  - // Copyright (c) 2026 Trent Polack. All Rights Reserved.
  - // Licensed under the MIT License.
- We use the oxford comma in this house, robot.
- Capitalize major words in Markdown headings.
- Before implementing or refactoring code, read, and follow [**Coding Standards**](docs/coding-standards.md), including its canonical style reference. Before completion, review all changed code against those requirements.
- For Rust and its WGSL, follow [**Rust Coding Standards**](docs/rust-coding-standards.md); `rustfmt` is authoritative for Rust layout.

## Commit Message Standards
- Don't use the "Conventional Commits" commit style; i.e., don't use the `feat(<branch>): <message>`, `chore: <message>`, etc..
- Commit messages must begin with a brief one-to-two sentence summary followed by `- ` bullet lines for subsequent details.
  - Notable changes to include in a commit message are important/significant changes to specific modules/features as well as the sort of change that'd warrant a `CHANGELOG.md` mention.
  - Use the past tense for changes that the commit is making.
- Group related changes into a few bullets describing meaningful outcomes. Omit minor implementation details, individual symbol changes, and routine documentation or test updates unless they materially affect behavior, compatibility, or workflow.
- Use markdown as-needed to improve the message display (e.g., use the `code` back-ticks for code names).
  - When committing through the command line, preserve the message literally and prevent shell expansion. Prefer writing the message to a temporary file and using `git commit --file <path>`. Markdown backticks must remain unescaped in the stored commit message.
