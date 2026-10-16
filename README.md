# Joy Engine
A game engine built on top of a WebGPU (WebGL 2.0 backup) renderer intended for prototyping, procedural generation, and other things that seem fun.

Browser-native runtime, rendering, particles, gameplay and development tooling shared by Joy Games and Joy Editor.

Runtime implementations and public contracts are authored as native TypeScript, including rendering, physics, environment simulation, FORM compilation, particles and in-game development overlays. Vite compiles browser builds; supported Node.js versions strip types for asset-free tooling entry points and tests. Strict no-emit checking validates native TypeScript and the remaining checked JavaScript consumers.

Browser games can use `GameShell` to compose configuration, a renderer, input, development diagnostics, ordered input/simulation/presentation/render phases, and dependency-ordered cleanup without moving project behavior into the engine.

The engine package is self-contained: it owns its TypeScript base configuration, tests, documentation and tooling, and imports nothing from the repository that consumes it. [Joy Editor](editor/README.md) lives in `editor/` as a separate package (with FORM LAB and PARTICLE LAB in `editor/tools/`); authoring UI and the Electron lab host belong to it, and the engine never imports it. It was developed inside [JoyGames](https://github.com/trentpolack/JoyGames), which now includes this repository as a git submodule at `joy-engine/`; see its [repository split](https://github.com/trentpolack/JoyGames/blob/develop/docs/development/repository-split.md) notes.

## Package commands
Run from this directory, or from the JoyGames root with `--workspace joy-engine`:

```bash
npm run check     # strict type check, TypeScript and Rust dependency-layer checks, script syntax gate
npm test          # engine unit suite in test/ (npm run test:engine from the JoyGames root)
npm run docs:api  # regenerate docs/api from the public exports
```

## Rust Engine
The repository is also the `joy-engine` Rust crate (`Cargo.toml` here, `src/lib.rs` as its root), with Rust modules beside the TypeScript in the same `src/` folders and dependency layers. Games and tools written against it run natively and in browsers (WebGPU with WebGL 2 fallback) from one codebase, and its renderer is the single renderer for games, FORM content, and tools. Joy Editor (`editor/`), FORM LAB, and PARTICLE LAB (`editor/tools/`) have Rust versions in the same Cargo workspace, and the examples have Rust entry points beside their TypeScript. The TypeScript engine keeps running the published games and tools until they move over; see the [Rust engine guide](docs/rust.md) for status, parity, and gaps.

```bash
cargo test                              # Rust unit, parity, and headless GPU tests
cargo test --workspace                  # plus Joy Editor, FORM LAB, and PARTICLE LAB
cargo run --example form-viewer         # FORM assets through the engine renderer
cargo run -p joy-editor                 # the native authoring workspace
npm run rust:web -- --example form-viewer   # browser build
```

## Package entry points
| Import | Purpose |
| --- | --- |
| `joy-engine` | Browser runtime: rendering, particles, gameplay, physics, input and audio. |
| `joy-engine/constants` | Data-only constants, safe for workers, editors and game runtime. |
| `joy-engine/form`, `joy-engine/levels`, `joy-engine/objects`, `joy-engine/project-manifest` | Asset document parsers and compilers for authoring tools and Node tests. |
| `joy-engine/scripting`, `joy-engine/particles/scripting` | Editor-neutral scripting contracts and built-in particle language information for authoring integrations. |
| `joy-engine/post-processing` | Node-safe post-processing configuration helpers (see below). |
| `joy-engine/development`, `joy-engine/development/editor`, `joy-engine/development/preview`, `joy-engine/dev/*` | Development-only in-game overlays and the editor preview bridge. |
| `joy-engine/development/vite` | Node-only Vite plugins: config tuning and post-processing persistence, shared favicon, and engine dependency pre-bundling. |
| `joy-engine/shaders/*`, `joy-engine/schemas/*` | Reusable authored WGSL/GLSL programs and JSON asset schemas. |
| `joy-engine/tsconfig.base.json` | Shared strict compiler options for engine consumers. |
| `joy-engine/testing/register-raw-loader` | `node --import` hook that lets Node load engine source with Vite-style `?raw` imports. |
| `joy-check-javascript` (bin) | Syntax gate for launchers, tooling and Node tests that TypeScript does not check. |

## Build-tool configuration
Node-loaded configuration files, including `vite.config.js`, must import the post-processing configuration helpers from the asset-free subpath:

```js
import { DEFAULT_POST_PROCESSING, normalizePostProcessingConfig, resolvePostProcessingProfile } from 'joy-engine/post-processing';
```

Do not import these helpers from the root `joy-engine` entry point in build-tool configuration. The root entry is the browser runtime and intentionally reaches CSS and shader `?raw` imports that Vite processes after it has loaded its config.
This also applies to transitive dependencies, such as game configuration loaded through tuning metadata. Profile validation, presets, and resolution are available from the same Node-safe entry point.

- [Engine guide](docs/README.md)
- [Rust engine guide](docs/rust.md)
- [Public API reference](docs/api/README.md)
- [Resident CLOD terrain](docs/terrain.md)
- [Scene effects and capture evidence](docs/scene-effects.md)
- [Particle authoring and runtime](docs/particles.md)
- [JoyGames repository documentation](https://github.com/trentpolack/JoyGames/blob/develop/docs/README.md)
