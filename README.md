[![Rust CI](https://github.com/trentpolack/joy-engine/actions/workflows/rust-ci.yml/badge.svg)](https://github.com/trentpolack/joy-engine/actions/workflows/rust-ci.yml)

# Joy Engine
Joy Engine is a Rust game engine for native applications and browser WebAssembly. It provides rendering, simulation, procedural content, audio, input, and authoring tools through the `joy-engine` crate, a facade over the engine's system crates in `crates/` (`joy-core`, `joy-physics`, `joy-gameplay`, `joy-form`, `joy-renderer`, `joy-app`, and others). Games and tools use one implementation on Windows, macOS, Linux, WebGPU, and the WebGL 2 fallback.

The renderer uses wgpu, the window and canvas shell uses winit, physics uses rapier, and the authoring interface uses egui. Consumers depend on `joy-engine` and its public exports in `src/lib.rs`, never on the system crates or backend crates directly. FORM EDITOR, JOYFX EDITOR, and [Joy Editor](editor/README.md) share that renderer and the engine's authoring kit.

## Setup and Commands
Requires Rust 1.95 or newer, Node.js 24.18.0 or newer for repository tooling, and the WebAssembly target for browser builds. Linux native builds need `pkg-config`, `libudev-dev`, and `libasound2-dev`; native GPU tests also need a Vulkan driver, such as Debian/Ubuntu's `mesa-vulkan-drivers`.

```bash
npm ci
rustup toolchain install nightly --profile minimal --component rustfmt
rustup target add wasm32-unknown-unknown
npm run check
npm test
cargo run --example form-viewer
cargo run -p joy-editor
npm run build:web -- --example form-viewer --release
npm run docs:api
```

Browser builds require the wasm-bindgen CLI version pinned by `Cargo.lock`: `cargo install wasm-bindgen-cli --version <lockfile-version> --locked`. `npm run docs:api` generates rustdoc locally; generated API output is not committed. Run `cargo doc --workspace --no-deps --open` to browse it.

Games using standard Lua 5.4 enable the facade's `game-script` feature. Lua is built through `mlua` with its `vendored` feature and a C compiler; browser builds additionally require WASI SDK 34 and `WASI_SDK_PATH`. Dependency versions, setup, and runtime integration are documented in [Game Scripts](docs/rust-game-script.md).

`npm run generate:lua-definitions` creates the shared LuaLS library directly from Rust bindings. `npm run watch:lua-definitions` keeps it updated while editing, and `npm run check:lua-definitions` checks freshness. See [Lua editor definitions](docs/rust-game-script.md#lua-editor-definitions) for registering APIs and extending the library from a game.

The engine owns its Cargo workspace, tests, assets, schemas, and build tooling. A hosting repository such as [JoyGames](https://github.com/trentpolack/JoyGames) consumes it as a submodule and provides its own site commands. npm workspaces remain useful for build orchestration, captures, metadata, and patch delivery.

## Public Entry Points and Features
Games and tools use the `joy-engine` facade (`src/lib.rs`): `run` and the `App` phases, `prelude`, `web_main!` for browser entry points, `game_assets!` and `source_root!` for the embedded asset table (`runtime::GameAssets`), `cooked_forms!` for FORM meshes cooked at build time, and the system modules (`core`, `math`, `physics`, `gameplay`, `form`, `particles`, `rendering`, `terrain`, `runtime`, and the rest). Cargo features decide what a build ships:

| Feature | Default | Adds |
| --- | --- | --- |
| `devtools` | Yes | `development`: the CONFIG and JOY-EDITOR dock, statistics overlay, console, config panels, live tuning, and effect editors, plus the shell's development UI. Without it, `Frame::dev_tools` is an inert `DevTools` and `runtime::development_dock_height` is zero. |
| `authoring` | No | The authoring kit, native file dialogs, and `project` manifests that Joy Editor, FORM EDITOR, and JOYFX EDITOR enable (implies `devtools`). |
| `game-script` | No | Sandboxed Lua game scripts (`game_script`). |
| `test-support` | No | Golden-file helpers (`testing`) for games' `[dev-dependencies]`. |

Games depend on the facade with `default-features = false` and turn `devtools` on through a default feature of their own, so `cargo run`, `npm run dev`, and tests keep the development tools while production builds pass `--no-default-features` and ship no development or authoring code. Games with FORM assets also cook them in a build script (`[build-dependencies] joy-cook`), so release and browser builds load cooked meshes instead of evaluating FORM; see [cooked FORM meshes](docs/rust-form.md#cooked-meshes).

## Guides
- [Documentation index](docs/README.md)
- [Runtime, rendering, and browser builds](docs/rust.md)
- [Coding standards](docs/coding-standards.md)
- [Repository audit](docs/repository-audit.md): `npm run audit:repo` and `npm run test:audit`.
- [Engine examples](examples/README.md)
- [Gameplay and physics](docs/gameplay.md) and [game scripts](docs/rust-game-script.md)
- [FORM](docs/rust-form.md), [particles](docs/particles.md), and [terrain](docs/terrain.md)
- [Authoring kit](docs/rust-authoring.md)
- [Changelog](CHANGELOG.md)
