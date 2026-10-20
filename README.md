[![Rust CI](https://github.com/trentpolack/joy-engine/actions/workflows/rust-ci.yml/badge.svg)](https://github.com/trentpolack/joy-engine/actions/workflows/rust-ci.yml)

# Joy Engine
Joy Engine is a Rust game engine for native applications and browser WebAssembly. It provides rendering, simulation, procedural content, audio, input, and authoring tools through the `joy-engine` crate, a facade over the engine's system crates in `crates/` (`joy-core`, `joy-physics`, `joy-gameplay`, `joy-form`, `joy-renderer`, `joy-app`, and others). Games and tools use one implementation on Windows, macOS, Linux, WebGPU, and the WebGL 2 fallback.

The renderer uses wgpu, the window and canvas shell uses winit, physics uses rapier, and the authoring interface uses egui. Consumers depend on `joy-engine` and its public exports in `src/lib.rs`, never on the system crates or backend crates directly. FORM LAB, PARTICLE LAB, and [Joy Editor](editor/README.md) share that renderer and the engine's authoring kit.

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

The engine owns its Cargo workspace, tests, assets, schemas, and build tooling. A hosting repository such as [JoyGames](https://github.com/trentpolack/JoyGames) consumes it as a submodule and provides its own site commands. npm workspaces remain useful for build orchestration, captures, metadata, and patch delivery.

## Guides
- [Documentation index](docs/README.md)
- [Runtime, rendering, and browser builds](docs/rust.md)
- [Coding standards](docs/coding-standards.md)
- [Repository audit](docs/repository-audit.md): `npm run audit:repo` and `npm run test:audit`.
- [Engine examples](examples/README.md)
- [Gameplay and physics](docs/gameplay.md)
- [FORM](docs/rust-form.md), [particles](docs/particles.md), and [terrain](docs/terrain.md)
- [Authoring kit](docs/rust-authoring.md)
- [Changelog](CHANGELOG.md)
