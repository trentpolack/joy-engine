# Runtime and Browser Builds
Joy Engine runs games and tools from one Rust codebase on Windows, macOS, Linux, and browser WebAssembly. Graphics use wgpu, windows and canvases use winit, physics uses rapier, and HUDs and authoring UI use egui. The published web builds use the same simulation and content code as native applications.

## Layout
`Cargo.toml` defines the engine workspace: the system crates in `crates/`, the `joy-engine` facade package, and the editor and labs. Shared package fields, dependency versions, and lints live in its `[workspace.*]` tables. `src/lib.rs` re-exports the system crates under their module names and exports the game-facing API, `prelude`, and `web_main!`. Games and tools depend only on `joy-engine`, never on the system crates or backend crates.

| Layer | Crates and modules | Responsibility |
| --- | --- | --- |
| Foundation | `joy-core` (`core`, `scripting`) | Math, transforms, colors, shapes, seeded randomness, and script language catalogs. |
| Simulation | `joy-environment` (`environment`), `joy-physics` (`physics`), `joy-gameplay` (`gameplay`) | Environment clocks, rigid bodies, vehicles, destruction, entities, controllers, and authored documents. |
| Content | `joy-form` (`form`), `joy-fx` (`particles`), `joy-dcc-gltf` (`dcc`), `joy-project` (`project`) | Procedural geometry, particle documents and simulation, glTF and GLB imports, and project manifests. |
| Rendering | `joy-renderer` (`rendering`, `particles` drawing), `joy-scene` (`rendering`) | Meshes, terrain, materials, lighting, sky and post-processing settings, shadows, effects, particle drawing, and captures; presentation components, post-process volumes, and level, asset, FORM, and environment presentation. |
| Platform | `joy-app`, `joy-input`, `joy-audio`, `joy-storage`, `joy-ui` (`runtime`), `joy-devtools`, `joy-authoring` (`development`) | Window/canvas shell and frame phases, input, audio, storage, egui UI and theme, the development dock, and authoring widgets, each with its browser backend. |

The [dependency checker](architecture/dependency-layers.md) validates these boundaries. Each system crate has its own integration tests (`crates/<crate>/tests/integration/`, run with `cargo test -p <crate>`); `tests/integration/` holds the facade's cross-crate integration tests; examples use `examples/<name>/main.rs` and project-local modules. Rust API documentation comes from `cargo doc --no-deps`.

## One Renderer
FORM returns renderer-free geometry. `compile_form_mesh`, `form_mesh_asset`, and `FormAssetLibrary` turn it into assets drawn by the same renderer as games, level previews, and tools. Meshes share immutable `MeshAsset` handles and use retained `MeshInstance`s in `RenderScene`; scene nodes group mesh and terrain transforms and visibility. GPU copies are released when their asset handles are no longer used.

A frame culls instances against the camera, renders each shadow view with only the casters inside it, fills depth in a pre-pass (terrain first, then opaque batches front to back), and shades the opaque scene into an HDR target in one pass that also writes scene-effect attachments when effects need them; the sky fills the remaining far-plane pixels. Transparency (blended meshes, transient triangles, and particles in one back-to-front queue) draws after the lighting effects and before temporal antialiasing, motion blur, exposure, and display processing. Frames may supply any number of point lights: the renderer draws the 32 that light the view most and reports the rest in `RendererDiagnostics::point_lights_omitted`. Lighting and blending happen in linear light; sRGB color maps and sprites are decoded when sampled (each with a generated mip chain, filtered trilinearly and anisotropically), and the display resolve encodes sRGB after tone mapping, so the presentation target receives display-encoded values (the grade, antialiasing, and film grain work on those encoded values). Colors authored as display values (hex strings, color pickers, and document colors) are decoded to `LinearColor` on input, so they display as authored. Lit, PBR, and Unlit materials have distinct contracts. PBR supports base color, normal, emissive, metallic, roughness, and occlusion textures, image-based lighting, and alpha-tested shadows. `TextureAsset` reads PNG, JPEG, and WebP images. Directional cascades and point-light shadows work on WebGL 2 as well as WebGPU.

Optional [scene effects](scene-effects.md) consume depth, surface, and motion data. [Terrain](rust-terrain.md), [particles](rust-particles.md), and `EditorGrid` share the renderer. Arcade projects may use `TriangleRenderer` with a project-owned WGSL program and shared display processing.

## Frame Phases
`joy_engine::run` owns the window or canvas, GPU startup, input, and development UI. It calls an `App`'s `input`, `simulation`, `presentation`, `render`, `ui`, and `after_frame` phases. Elapsed time is seconds, clamped to 0.1. Hidden windows reset the clock and notify `visibility_changed`; focus loss releases held input. A phase error stops the loop and presents the failure.

The shell is the renderer's only clock: before each render phase it hands the frame's time to `Renderer::set_presentation_time` (and `RenderTarget::draw_triangles` copies it to a `TriangleRenderer`), so temporal antialiasing history resets, exposure adaptation, film grain, and volumetric noise behave the same in every game and keep running while a game pauses. Scene animation stays on the app's own clock. Renderers driven without a shell (tests and offline captures) advance the clock themselves, for example with `PresentationTime::fixed_rate`.

Games, tools, and examples all implement `App`. Native close requests call `App::close_requested`; returning false lets an editor protect unsaved work. `Frame::request_exit` closes later. `HeadlessShell` runs these phases offscreen at a fixed step for tests; `step_with_ui` adds synthetic egui input and UI rendering for captures.

Use `core::FixedStepLoop` for simulations tuned to fixed ticks, and draw between ticks with `FixedStepLoop::interpolation` (physics-driven entities use `World::presented_entities` and `World::presentation_transform`; see [simulation](rust-simulation.md)), so motion stays smooth when the display refreshes faster than the simulation. Keep cosmetic randomness separate from gameplay, and use `SeededRandom` for repeatable runs. Portable numerical helpers preserve deterministic simulation across native and WebAssembly targets. `runtime::launch_parameter` reads browser query parameters or native `--name=value` arguments.

## Browser Builds
A game's library is a `cdylib` and exposes its startup with `joy_engine::web_main!(start)`. The Node build wrapper compiles WebAssembly, invokes the lockfile-matched wasm-bindgen CLI, and emits the page and browser bindings. These generated bindings are required browser infrastructure.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <lockfile-version> --locked
npm run build:web -- --example form-viewer --release
node scripts/rust/build-web.mjs --package form-lab --out <folder> --release
```

Workspace `build` scripts create release browser output in `dist/`; `--dev` selects a faster development build. JoyGames' root `npm run dev` builds and serves projects with live rebuilds, and root `npm run build` assembles their WebAssembly pages at the canonical game/tool/example routes.

Browser rendering chooses WebGPU when available and falls back to WebGL 2. Append `?backend=webgpu` or `?backend=webgl` to require a backend. egui uses the engine's browser input translator, and backing resolution follows measured canvas density.

## Validation
Rust 1.95 or newer is required. On Debian/Ubuntu, install `pkg-config`, `libudev-dev`, and `libasound2-dev` for native gamepads and audio. Native GPU tests also need a Vulkan driver; `mesa-vulkan-drivers` supplies Mesa's hardware and software Vulkan implementations. The OpenGL fallback lacks the scene effects required by some renderer tests.

```bash
rustup toolchain install nightly --profile minimal --component rustfmt
cargo +nightly fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown --lib --examples
cargo run --example form-viewer
cargo run -p joy-editor
```

Headless GPU tests skip with a diagnostic when no adapter is available; they can also use software adapters. Set `JOY_RENDER_CAPTURE_DIR` to retain captures. The [Rust CI workflow](../.github/workflows/rust-ci.yml) runs formatting/Clippy, native tests, and a locked WebAssembly check. A host's game tests complement rather than replace the engine's own suites.

Recorded reference fixtures preserve compatibility checks without requiring a second runtime. Solver behavior, authored-data contracts, and platform limitations are documented in the subsystem guides. Browser audio requires a user gesture; the static editor build offers bundled examples and downloads, while project filesystem authoring requires the native editor.
