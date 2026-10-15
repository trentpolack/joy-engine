# Rust Engine
Joy Engine has a Rust implementation in `crates/`, built so games and tools run natively (Windows, macOS, Linux) and in browsers (WebGPU, with WebGL 2 fallback) from one codebase, without Electron. It uses [wgpu](https://wgpu.rs) for graphics, winit for windows and canvases, rapier for physics, and egui for HUDs and development UI, at the same versions as joy-apps. The TypeScript engine in `src/` remains the shipping runtime and the behavioral reference until the Rust engine covers what a game needs; both read the same `.joylevel`, `.form`, `.formlab`, and post-processing profile files.

## Crates
Each crate owns one layer from `architecture/layers.json`, so the crate graph enforces the same one-way model as the TypeScript boundary checker. `npm run check` runs `scripts/check-rust-layers.mjs`, which reads the manifests (no Rust toolchain needed) and rejects upward edges and backend crates outside their owning layers.

| Crate | Layer | Owns |
| --- | --- | --- |
| `joy-foundation` | foundation | Math, transforms (XYZ Euler, Y-up), colors, primitive shapes, `SeededRandom` (bit-identical to TypeScript), and post-processing profiles. |
| `joy-simulation` | simulation | `EnvironmentSystem`, rapier rigid bodies and raycast vehicles, entities, behaviors, controllers, `World`, `LevelRuntime`, level and object documents, and grid navigation. Headless. |
| `joy-form` | content | The FORM language: parser, evaluator, and resources, returning renderer-free `GeometryData`. |
| `joy-render` | rendering | The renderer every game, tool, and preview draws with, plus `LevelScene` and `FormAssetLibrary`. |
| `joy-platform` | platform | The game shell: window or canvas, GPU startup, frame phases, input, egui HUD and dev overlay, and headless runs. |
| `joy-engine` | facade | Re-exports the layers, a `prelude`, and `web_main!`. Games depend on this crate only. |

Games, tools, and examples never depend on wgpu, winit, rapier, or egui directly (egui is re-exported for UI). This is the Rust form of the rule that consumers never import luma.gl.

The port also retires the TypeScript engine's recorded layer violations by construction: level presentation (`LevelScene`) moved into the rendering layer, FORM no longer reaches into rendering, and simulation types carry no rendering records.

## One Renderer
FORM has no renderer of its own. FORM produces geometry; `joy-render` turns it into meshes (`compile_form_mesh`, `form_mesh_asset`, `FormAssetLibrary`) and draws them exactly as it draws level primitives, so the editor, FORM tools, previews, and games show the same lighting, shadows, sky, and display processing. The `form-viewer` example is the reference integration.

The renderer follows the TypeScript frame so content looks the same:
1. Frustum culling and a directional shadow map (3×3 PCF, fitted around the camera focus and nearby casters, texel-snapped), on WebGL 2 as well as WebGPU.
2. An HDR scene pass (`Rgba16Float` where renderable, filterable, and blendable): procedural atmosphere sky, opaque batches sorted by pipeline and mesh, transient opaque triangles, then transparent instances and transient triangles back to front.
3. Material passes: Lit (the engine's flat, double-sided vertex-color model), PBR (metallic-roughness GGX), and Unlit. These stay separate because the TypeScript Lit model is not PBR (no 1/π and a two-sided `|n·l|`).
4. Display processing in TypeScript order: bloom, full blur, vignette blur, AgX/ACES/linear tone mapping, hue/saturation, brightness/contrast, vibrance, vignette, antialiasing, and film grain, driven by the same `PostProcessingConfig` values.

Meshes are shared `MeshAsset`s (vertex color, normals, and baked material factors) drawn by `MeshInstance`s in a retained `RenderScene`; `MeshCollection` keys instances to simulation records. The renderer owns GPU resources and releases a mesh's buffers when no instance or asset handle references it.

## Frame Phases
`joy_platform::run` (re-exported as `joy_engine::run`) owns the window or canvas, device, renderer, input, and dev tools, and calls a `Game`'s phases each frame: `input`, `simulation`, `presentation`, `render`, `ui`, then `after_frame`, matching `GameShell`. Elapsed time is in seconds and clamped to 0.1; hiding the window resets the clock and calls `Game::visibility_changed`, and focus loss releases held input. A phase error stops the loop and shows the failure. `HeadlessShell` runs the same phases offscreen with a fixed step for tests and captures.

Input uses DOM `KeyboardEvent.code` names, so bindings read like TypeScript ones. Touch points are reported individually (`InputManager::touches`) so on-screen controls can be held together, for example steering while accelerating.

## Browser Builds
The same crate compiles to `wasm32-unknown-unknown`. A game exposes its entry with `joy_engine::web_main!(start)` in a `cdylib` library; `scripts/rust/build-web.mjs` builds it, runs the wasm-bindgen CLI pinned by `Cargo.lock`, and writes a static page:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <lockfile version> --locked
npm run rust:web -- --package joy-engine --example form-viewer --release
```

The browser picks WebGPU when available and falls back to WebGL 2; `?backend=webgpu` or `?backend=webgl` forces one. Browsers use the engine's own egui input translator because the pinned egui-winit does not build for WebAssembly. Layout uses the canvas's measured backing ratio, which stays correct when a browser's `devicePixelRatio` and canvas measurements disagree.

## Commands
Run from this directory (the Cargo workspace root):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                       # GPU tests skip without an adapter
cargo run -p joy-engine --example form-viewer
npm run rust:web -- --package joy-engine --example form-viewer
```

Headless GPU tests run on software adapters too (llvmpipe or SwiftShader through `VK_ICD_FILENAMES`). Set `JOY_RENDER_CAPTURE_DIR` to keep their PNG captures. FORM parity against the TypeScript compiler runs with `npm run test:rust-parity --workspace @joy-games/form-lab` after `cargo build --release -p joy-form --bin joy-form-evaluate`.

See [Rust coding standards](rust-coding-standards.md) for conventions.

## Parity and Known Differences
Verified against the TypeScript engine: seeded random streams, transforms, environment frames (seven recorded cases within 1e-5), FORM output (all 69 FORM LAB corpus cases), level documents round tripping byte for byte, grid routes, and render captures of the same scenes (sky tone and lit ground within a few 8-bit levels).

- **AgX display output.** The TypeScript display path writes AgX's linearized output into a non-sRGB canvas without encoding it, so AgX looks darker than ACES there. The Rust renderer reproduces this for parity; correcting it should be a deliberate change in both engines.
- **Physics.** Rapier replaces cannon-es. Driving is close but not identical; see the `joy-simulation` README for the full list. One visible case: the VEHICLE PLAYGROUND kart's authored tuning wheelies and backflips under sustained full throttle in both engines, after about 1.0 second in Rust and about 2.3 seconds in TypeScript.
- **Not yet ported:** particles and JOYFX, terrain, scene effects (SSAO, TAA, depth-of-field), image-based lighting and textures, point-light shadows, audio, gamepads, DCC import, scripting, destruction, live tuning, and the post-processing editor. FORM LAB, PARTICLE LAB, and Joy Editor remain TypeScript applications.

## Migration Path
1. Port games as Rust crates beside their TypeScript projects (VEHICLE PLAYGROUND is the first: `projects/vehicle-playground/rust/` in JoyGames).
2. Port the subsystems those games need from the list above into the owning crates, with parity tests against the TypeScript reference.
3. Move tools (FORM LAB first) onto `joy-render` and `joy-platform` once the renderer covers their previews, so there is one renderer across games and tools.
4. Retire each TypeScript subsystem once nothing uses it.
