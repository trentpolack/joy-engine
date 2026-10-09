# AGENTS.md
This file applies to the entire repository unless a package-specific `AGENTS.md` says otherwise for that package.

## Overview
Joy Engine is a browser-native game engine built on luma.gl (WebGPU with WebGL 2 fallback) for prototyping, procedural generation and stylized games. This repository holds the engine package, Joy Editor and its FORM LAB and PARTICLE LAB tools, and command-line engine examples.

Prefer readable control flow, named data, and small modules over clever abstractions or compression. If a change makes code harder to scan, explain why the tradeoff is worthwhile. There are additional specific cases in the "Code Standards and AI Agent Rules" section of this doc.

## Setup and validation
When working in this repository on its own, run commands from its root. When it is included in a larger workspace, follow that workspace's workflow and use its existing commands to edit and validate changes across directories:
```bash
npm ci
npm run check
npm test
npm run check --workspaces --if-present
npm test --workspaces --if-present
```

`npm run check` and `npm test` cover the engine package; the `--workspaces` forms cover Joy Editor, FORM LAB, PARTICLE LAB and the examples. Run `npm run docs:api` whenever public exports, types or their documentation change, and commit the regenerated `docs/api/`.

Before completing a change:
1. Run the narrowest relevant check while iterating.
2. Run the engine and workspace checks and tests above.
3. Build the affected workspace (`npm run build --workspace <name>`).
4. For rendering, gameplay-facing or tool UI changes, also run the available hosting workspace's integration validation and browser captures. Record browser limitations and capture failures explicitly.

Joy Editor's development server and browser captures run inside a hosting workspace; see [the editor README](editor/README.md#package-and-hosting-workspace).

Do not commit `node_modules`, generated `dist` folders, logs, caches, or local environment files (i.e., follow the `.gitignore` rules).

## Assets and Intellectual Property/Trademarks
- Treat external games as references for principles and feel, never as sources for copied code, art, audio, layouts, or exact effects.
- When ownership or redistribution rights are unclear, stop, and ask.

## Change Discipline
Read the root README and the affected package's README before making structural changes. Preserve unrelated user work and avoid broad rewrites outside the requested package.

When this repository is included as a submodule in a larger workspace, treat it as a directory for the requested edits and validation. Do not introduce a separate repository handoff or require Git state changes to complete the task. Leave submodule commits, branches, pushes, merges, tags, and the parent pin to the user unless explicitly requested; this applies to local, IDE, and cloud work.

Keep the engine's public API small and document why new shared behavior belongs in `src/`. For editor- or tool-only changes, avoid touching the engine package or root tooling unless the change cannot be supported cleanly otherwise.

In addition:
- Update the root-level `CHANGELOG.md` whenever a major/minor/patch change is made; don't get too granular, just calling out major changes.
- Group changelog entries under one `## MM-DD-YYYY` heading per day, newest first, with `###` headings for projects or topics. Reuse the day's existing heading rather than creating duplicate date sections. Use the completion date in `America/Detroit` for current work and the introducing commit's date in that timezone for historical backfills. Do not add `Unreleased` sections or replace known historical dates with today's date. Automated changelog updates must preserve existing entries and this daily structure.
- When encountering new/found work, bugs, potential future work, deprecated logic/packages, adding tech debt then create an Issue in GitHub.
  - Don't create "checklist" issues; just file actionable issues.
  - For follow-up/future work, create a GitHub Issue; do not create standalone markdown files like `follow-ups.md`.

Use the completion date in `America/Detroit`. Keep the whole summary under roughly 300 words; link out instead of expanding.

## Unit Test Scope
Keep the unit suite focused on core behavior and costly regressions: document and asset round trips, bounded or deterministic simulation, resource ownership and cleanup, backend contracts and fallback, and gameplay state transitions. Extend an existing scenario when it already covers the same risk. Add a new test only when it protects a distinct failure that matters to a player, author, or shared-engine consumer.

Do not add tests solely for coverage counts, trivial helpers, fixed tuning values, visual styling, source-text patterns, or implementation details already exercised through behavior. Review appearance and game feel with the existing browser checks and captures. Preserve regression cases for data loss, resource leaks, runaway work, and compatibility failures.

## Workspace structure
- `src/`, `shaders/`, `schemas/`, `assets/`: the `joy-engine` package (public entry points are listed in [README.md](README.md)).
- `test/`, `scripts/`, `docs/`: the engine's unit suite, tooling, guide and generated API reference.
- `editor/`: Joy Editor (`@joy-games/joy-editor`), a separate package with its own dependencies; FORM LAB and PARTICLE LAB live in `editor/tools/`.
- `examples/*`: command-line engine examples, outside any published site.

The engine package never imports the editor, tools or examples. The editor may depend on the engine (`file:..`). Tests, fixtures and tooling live inside the owning package. Nothing in this repository may depend on a particular game; game-specific fixtures belong in the consuming repository.

Move code into `src/` when it is stable behavior that more than one game, tool, or example needs.

## Code Standards and AI Agent Rules
- Follow [the detailed JavaScript coding standards](docs/coding-standards.md). They adapt the supplied `joygl-ts` (a previous project) client's staged initialization, renderer member grouping, explicit disposal, and API documentation to instance-owned browser-native JavaScript. Apply these rules to changed code, not just new documentation.
- Always use [JoyCore](https://github.com/trentpolack/JoyCore) as a reference for code organization, commenting style, naming, clarity, and overall readability. Consult relevant examples when implementing or refactoring code, and adapt their principles to this repository's language and browser-native architecture, even when JoyCore uses a different language or engine.
- Write browser-native TypeScript or JavaScript modules and use descriptive names.
- Prefer `.ts` files and native TypeScript contracts for reusable runtime source. JavaScript remains appropriate for build scripts, configuration, and small browser composition roots; describe its contracts with JSDoc. Strict TypeScript checking runs with `noEmit`; Vite owns production output.
- Keep shared numeric contracts in `src/core/types.ts`, with public type aliases in `src/index.ts`. Game data and presentation contracts belong inside each consuming game, tool or example.
- Extend `tsconfig.base.json` for each new workspace (example or tool) and include its type check in that workspace's `check` command. Engine imports, including type-only JSDoc imports, must not depend on the editor, tools, examples or any game.
- Keep all common/engine functionality in `src/` and consume it through `joy-engine`; do not add parallel implementations or compatibility packages.
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
- Games, the editor, tools and examples must consume engine functionality through `joy-engine` package exports. Do not import engine internals by relative path or copy engine modules into consumers. No consumer should directly rely on `luma.gl`; it should route through `joy-engine`.
- Before changing an exported engine contract, locate all consumers, including JSDoc types, tests, fixtures, and documentation. Update affected consumers in the same change. Prefer a coordinated migration over permanent compatibility layers.
- Preserve existing gameplay behavior during engine extraction and refactoring. Preserve default values, time units, update order, random-number consumption, collision semantics, and rendering order unless the task explicitly changes them. Separate behavior changes from structural changes.
- Consumers pin a commit or release tag of this repository. When explicitly asked to tag a release, use `vX.Y.Z` from the engine package version. Keep the release branch releasable, and call out breaking changes to exported contracts in the CHANGELOG. Do not create private engine copies to freeze a consumer.
- Document time units, coordinate spaces, ownership, mutation, and resource lifecycle at shared API boundaries. Make seconds versus milliseconds and per-tick versus per-second values explicit.
- Shared systems must support independent instances and explicit cleanup. Avoid mutable module-level gameplay state. Owners must release event listeners, timers, audio resources, and GPU resources on disposal.
- Keep gameplay randomness separate from cosmetic randomness. Support seeded or injected randomness where reproducible simulation is needed so visual changes do not alter gameplay outcomes.
- Always aim to build out the core engine (`src/`) with functionality that can benefit all current and future projects.
- Keep a consistent DPI/resolution for projects; ensure the 3D scene and HUD/UI elements have the proper scale for standard or high DPI displays. A higher resolution/DPI should not materially change how much the player can see.
- Aim for quality rendering; stylized does not have to mean compromised fidelity or effects.
- HTML5/CSS/Canvas can be used (but does not have to be) for HUD/UI elements but everything else in the game should be rendered in a proper 3D scene, with HDR if supported, with joy engine serving as a wrapper over luma.gl. Additionally, projects should have no need for direct luma.gl code unless it's truly a one-off (and then get approval before executing).
  - Ensure that there is logic for rendering fallbacks; target WebGPU with a fallback to WebGL 2 if unsupported; even potentially fallback to WebGL 1 if neither WebGPU or WebGL 2 is supported.
- Favor explicit ownership, small modules, and clear state transitions. Use object-oriented patterns when they improve clarity, but do not force them where a simpler functional or data-driven shape is clearer.
- Keep code clear, well-formatted, and documented (in-line/commented).
- Organize JavaScript for a C/C++-friendly reading model: make ownership and lifetimes explicit, keep composition roots procedural, use classes for stateful owners, and treat plain records as structs. Prefer early returns, descriptive intermediate values, and named functions over dense callback chains or clever expressions.
- Keep public methods and the main control path near the top of a module; move narrow implementation helpers below them. Group related state and avoid hidden mutation, implicit singleton state, and abbreviations that make call sites harder to scan.
- Keep authored GPU programs in standalone `.wgsl` and `.glsl` files. JavaScript may select, load, and bind shaders, but should not contain shader implementations as template literals.
- Development entry points should expose useful startup, backend, lifecycle, and failure diagnostics without producing routine logs in production builds. Reusable frame/runtime inspection belongs in Joy Engine and should remain removable from production sessions.
- Verify development behavior through the actual npm entry point. Root and project `dev` commands must serve development modules; production preview is explicitly named `preview`. Test production silence separately from development diagnostics.
- Test for Safari, Chrome, and Edge.
- Don't put a space after `if`, `for`, `while` keywords.
- Put all `if` logic in braces (and not on a single line).
- Don't add unnecessary trailing commas for containers/initializers.
- When using operators that adjust and set a variable (`+=`, `-=`, `*=`, etc.) don't add a space; e.g., it should be: `x+= 1`.
- Don't use spaces around exponents, multiplication, or division operators; addition and subtraction should have spaces between two parameters.
- When using scalars, don't use decimals when the expected operation is essentially between integers. Use decimals when it's assumed that they're floating-point values.
- When writing JSDoc-style function comments, the description for the function should be a new line after the initial `/*`.
- For clarity when multiple operations are happening, I like to make the order of operations clear with parentheses that may be _functionally_ pointless but improve readability.
- Games built on the engine need the standard CONFIG button/menu and JOY-EDITOR config button/menu; the engine owns those shared development widgets.
- Always avoid hard-coded strings that have gameplay/logic/engine relevance; prefer to create a const variable for the string and use _that_ elsewhere.
- Comment code and embrace the comfort of some extra line breaks when it comes to separating "chunks" of code; check out my general style of commenting/spacing in my JoyCore project: https://github.com/trentpolack/JoyCore/tree/develop/Source/JoyCore/Private/Systems; this engine uses JavaScript/TypeScript and has different conventions, but I'm a C++/C# person so this is the style I'd bring to JS projects.
- The header copyright/MIT license text for each file being created should be the copyright on one line and the license on the next line; i.e.:
  - // Copyright (c) 2026 Trent Polack. All Rights Reserved.
  - // Licensed under the MIT License.

## Commit Message Standards
- Don't use the "Conventional Commits" commit style; i.e., don't use the `feat(<branch>): <message>`, `chore: <message>`, etc..
- Instead, the commit message should have a summary as its opening line and then a list of relevant changes (prefixed with `-` to preserve list presentation.
  - Notable changes to include in a commit message are important/significant changes to specific modules/features as well as the sort of change that'd warrant a `CHANGELOG.md` mention.
