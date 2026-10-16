# Rust Engine
Joy Engine is becoming a Rust engine so games and tools run natively (Windows, macOS, Linux) and in browsers (WebGPU, with WebGL 2 fallback) from one codebase, without Electron. It uses [wgpu](https://wgpu.rs) for graphics, winit for windows and canvases, rapier for physics, and egui for HUDs and development UI, at the same versions as joy-apps. Until every consumer moves over, the TypeScript engine keeps running the published games and tools, and both read the same `.joylevel`, `.form`, `.formlab`, and post-processing profile files.

## Layout
The repository is one Cargo package, `joy-engine`: `Cargo.toml` at the root and the crate root at `src/lib.rs`. Rust modules live in the same `src/` folders as their TypeScript counterparts, so `src/physics/physics_world.rs` sits beside `src/physics/physics-world.ts`. TypeScript files leave as their Rust replacements take over their consumers.

| Layer | Folders | Rust contents |
| --- | --- | --- |
| foundation | `src/core` | Math, transforms (XYZ Euler, Y-up), colors, primitive shapes, `SeededRandom` (bit-identical to TypeScript), JavaScript `Math` semantics for deterministic simulations (`js_math`), and post-processing profiles. |
| simulation | `src/environment`, `src/physics`, `src/gameplay` | `EnvironmentSystem`, rapier rigid bodies, raycast vehicles (with live tuning), and destructible chunk structures, entities, components, behaviors, controllers, `World`, `LevelRuntime`, level and object documents with undo, component schemas, and grid navigation. Headless. |
| content | `src/form`, `src/dcc`, `src/project`, `src/scripting` | The FORM language (parser, evaluator, procedural geometry, and resources) returning renderer-free `GeometryData`, GLB mesh import, project manifests, and script language information. |
| rendering | `src/rendering`, `src/particles` | The renderer every game, tool, and preview draws with: scenes, materials, shadows, image-based lighting, terrain, scene effects, bloom and display processing, the arcade triangle renderer, captures, `LevelScene`, `AssetScene`, `FormAssetLibrary`, and the editor grid; plus particles and JOYFX. WGSL lives in `src/rendering/shaders/` (scene effects keep luma.gl's passes in `src/rendering/scene_effects/shaders/`). |
| platform | `src/runtime`, `src/development`, `src/browser` | The game shell (window or canvas, GPU startup, frame phases, fixed-step loops, input, audio, storage, launch parameters, egui UI layer, headless runs), the development dock (statistics, post-processing editor, scene effect controls, config panels, live tuning), the authoring kit for tools (lab controls, property history, panel layout), and WebAssembly browser glue. |

`src/lib.rs` is the public API: the layer modules, top-level re-exports for games (`run`, `Game`, `ShellOptions`, …), a `prelude`, and `web_main!`. `src/bin/joy-form-evaluate.rs` is the FORM parity CLI, `examples/<name>/main.rs` files are Rust examples (form viewer, terrain, scene effects, FORM building, and FORM destruction) with their modules beside the TypeScript in each example's `src/` (see [examples](../examples/README.md)), and `test/rust/` is one integration-test binary beside the TypeScript tests.

Subsystem guides: [simulation](rust-simulation.md), [FORM](rust-form.md), [content](rust-content.md), [particles](rust-particles.md), [terrain](rust-terrain.md), [storage](rust-storage.md), and the [authoring kit](rust-authoring.md) that FORM LAB, PARTICLE LAB, and Joy Editor share.

Layers are enforced on Rust the same way as on TypeScript: `npm run check` (`scripts/check-boundaries.mjs`) reads every `crate::<module>` path, including grouped `use` imports and doc links, against `architecture/layers.json`, and limits backend crates (`rustBackendCrates`) to their owning layers: wgpu to rendering and platform, rapier to simulation, and winit and egui to platform. Games, tools, and examples never depend on those crates directly (egui is re-exported for UI), the Rust form of the rule that consumers never import luma.gl.

The Rust modules also retire the TypeScript engine's recorded layer violations by construction: level presentation (`LevelScene`) is in the rendering layer, FORM never reaches into rendering, post-processing profiles are foundation data, and simulation types carry no rendering records.

## One Renderer
FORM has no renderer of its own. FORM produces geometry; `rendering` turns it into meshes (`compile_form_mesh`, `form_mesh_asset`, `FormAssetLibrary`) and draws them exactly as it draws level primitives, so the editor, FORM tools, previews, and games show the same lighting, shadows, sky, and display processing. The `form-viewer` example is the reference integration.

The renderer follows the TypeScript frame so content looks the same:
1. Frustum culling and shadows ported from the luma.gl shadow module TypeScript uses: three or four texel-snapped directional cascades with percentage-closer soft shadows, cascade blending, and a far fade (receivers offset by at least 1.5 cascade texels, where TypeScript's fixed 0.01 bias leaves rings of self-shadowing on distant flat ground), plus six-face depth maps for every point light set to cast (up to 32). Point-light faces live in a 2D depth array rather than a cube array, and shadows also run on WebGL 2 (TypeScript draws none there); the GL backend filters comparisons in the shader because WebGL 2 cannot sample depth arrays in comparison mode at an explicit level.
2. An HDR scene pass (`Rgba16Float` where renderable, filterable, and blendable): procedural atmosphere sky, opaque batches sorted by pipeline and mesh, transient opaque triangles, then transparent instances and transient triangles back to front.
3. Material passes: Lit (the engine's flat, double-sided vertex-color model), PBR (metallic-roughness GGX), and Unlit. These stay separate because the TypeScript Lit model is not PBR (no 1/π and a two-sided `|n·l|`). PBR submeshes may carry `MaterialTextures` (base color, normal, emissive, metallic, roughness, and occlusion maps) and receive image-based lighting from an `EnvironmentMap` that `Renderer::prepare_environment_map` integrates from an equirectangular image (a port of luma.gl's generator: diffuse irradiance, GGX-prefiltered specular mips, and a BRDF table), passed per frame through `FrameInputs::image_lighting`. `MeshAsset::from_pbr_mesh` turns the TypeScript `PbrMesh` document into such an asset (and `form_mesh_asset` turns FORM geometry whose materials embed maps into one); where TypeScript draws one renderer-owned material mesh, Rust draws any number of instanced textured assets, and masked materials cast alpha-tested shadows. `TextureAsset` decodes PNG, JPEG, and WebP files and `data:` URLs on every target. Tools that share one renderer change the sky (`Renderer::set_atmosphere`) and shadow settings (`Renderer::set_shadow_options`, for example a distance fitted to a preview camera) between frames.
4. Display processing in TypeScript order: bloom, full blur, vignette blur, AgX/ACES/linear tone mapping, hue/saturation, brightness/contrast, vibrance, vignette, antialiasing, and film grain, driven by the same `PostProcessingConfig` values. Bloom ports the fragment path of luma.gl's `createBloomShaderPassPipeline` with the TypeScript options (two to five pyramid levels, bicubic reconstruction, anamorphic radii, and horizontal lens streaks), so it matches TypeScript on WebGL; TypeScript on WebGPU downsamples with luma.gl's compute pyramid instead, which differs slightly in the lower levels.

Meshes are shared `MeshAsset`s (vertex color, normals, and baked material factors) drawn by `MeshInstance`s in a retained `RenderScene`; `MeshCollection` keys instances to simulation records. Scene nodes (`RenderScene::insert_node`) group mesh and terrain instances into transform and visibility hierarchies, replacing TypeScript's parented luma.gl groups. The renderer owns GPU resources and releases a mesh's buffers when no instance or asset handle references it.

The same renderer draws the rest of the TypeScript frame:
- **Scene effects** (`FrameInputs` plus `Renderer::set_scene_effects`): SSAO or GTAO, screen-space global illumination and reflections, outlines, height fog, clustered volumetric lighting, depth-aware blur, temporal antialiasing, motion blur, and adaptive exposure, from luma.gl's shader passes as in TypeScript. `development::SceneEffectsControls` is the preview panel.
- **Terrain** through `RenderScene` terrain instances, lit and shadowed like meshes (see [Rust terrain](rust-terrain.md)).
- **Particles** in the transparent queue, with particle point lights (see [Rust particles](rust-particles.md)).
- **Arcade games** through `TriangleRenderer`, which draws game-built triangle lists with a game-supplied WGSL shader and the shared display processing.
- **Editor reference grid** (`EditorGrid`), camera-facing translucent ribbons on the XZ plane.

## Frame Phases
`joy_engine::run` (`src/runtime/shell.rs`) owns the window or canvas, device, renderer, input, and dev tools, and calls a `Game`'s phases each frame: `input`, `simulation`, `presentation`, `render`, `ui`, then `after_frame`, matching `GameShell`. Elapsed time is in seconds and clamped to 0.1; hiding the window resets the clock and calls `Game::visibility_changed`, and focus loss releases held input. Closing a native window asks `Game::close_requested` first; returning `false` keeps it open (an editor asking about unsaved work), and `Frame::request_exit` closes it later. A phase error stops the loop and shows the failure. `HeadlessShell` runs the same phases offscreen with a fixed step for tests and captures; `step_with_ui` adds the UI phase with synthetic input and paints it into the capture.

Input follows the TypeScript `InputManager`: DOM `KeyboardEvent.code` names for keys, the W3C standard mapping for gamepads (gilrs natively, `navigator.getGamepads()` in browsers), actions held above 0.15, axes that take the stronger of their digital value and a rescaled analog stick, contexts (`set_contexts`), and `rebind`. Focus loss, hidden windows, and egui text entry suspend input, and gamepads must return to neutral after any release. Touch points are reported individually (`InputManager::touches`, or `touch_points` with each finger's id so a stick can follow the finger that started on it) so on-screen controls can be held together, for example steering while accelerating; touches also hold the primary mouse button, as browser pointer events do. TypeScript's immediate `onPress` callback has no equivalent: Rust games read `frame.input.pressed(name)` each frame. Games that steer with the mouse call `Frame::set_pointer_lock(true)` (from a user gesture in browsers); while the lock holds, `InputManager::pointer_motion` returns the frame's raw mouse movement in logical pixels and `pointer_locked` reports the lock, which the browser or window may release at any time.

`AudioBank` plays WAV or raw PCM sounds (procedurally synthesized ones included) with the TypeScript voice limits and voice stealing. Native builds mix on the default output device through cpal; browsers use Web Audio and resume on the page's next pointer, touch, or key press. Without an audio device the bank is silent and `play` returns false. `set_master_volume` scales sounds as they start, for a game's volume setting (the TypeScript games route voices through a master gain node instead).

## Browser Builds
The same crate compiles to `wasm32-unknown-unknown`. A game exposes its entry with `joy_engine::web_main!(start)` in a `cdylib` library; `scripts/rust/build-web.mjs` builds it, runs the wasm-bindgen CLI pinned by `Cargo.lock`, and writes a static page:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <lockfile version> --locked
npm run rust:web -- --example form-viewer --release
```

Games, the published examples, FORM LAB, PARTICLE LAB, and Joy Editor each declare a `rust:web` npm script that runs this build with `--release`; a hosting site passes `--out <folder>` (JoyGames' `npm run build:rust` stages them in `dist-rust/` and publishes them beside or in place of the TypeScript builds). `--dev` overrides a script's `--release` for fast development builds; JoyGames' `npm run dev:rust` serves every Rust page that way with live rebuilds.

The browser picks WebGPU when available and falls back to WebGL 2; `?backend=webgpu` or `?backend=webgl` forces one. Browsers use the engine's own egui input translator because the pinned egui-winit does not build for WebAssembly. Layout uses the canvas's measured backing ratio, which stays correct when a browser's `devicePixelRatio` and canvas measurements disagree.

## Commands
Run from this directory (the package root). Linux builds need the udev and ALSA development packages for gamepads and audio (`sudo apt install pkg-config libudev-dev libasound2-dev` on Debian and Ubuntu).

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test                                   # the engine; GPU tests skip without an adapter
cargo test --workspace                       # the engine plus Joy Editor, FORM LAB, and PARTICLE LAB
cargo run --example form-viewer
cargo run -p joy-editor                      # also form-lab and particle-lab
npm run rust:web -- --example form-viewer
node scripts/rust/build-web.mjs --package form-lab --canvas form-lab --title "FORM LAB"
```

Headless GPU tests run on software adapters too (llvmpipe or SwiftShader through `VK_ICD_FILENAMES`). Set `JOY_RENDER_CAPTURE_DIR` to keep their PNG captures. FORM parity against the TypeScript compiler runs with `npm run test:rust-parity --workspace @joy-games/form-lab` after `cargo build --release --bin joy-form-evaluate`.

See [Rust coding standards](rust-coding-standards.md) for conventions.

## Parity and Known Differences
Verified against the TypeScript engine: seeded random streams, transforms, environment frames (seven recorded cases within 1e-5), FORM output (all 69 FORM LAB corpus cases), level documents round tripping byte for byte, grid routes, and render captures of the same scenes (sky tone and lit ground within a few 8-bit levels).

- **AgX display output.** The TypeScript display path writes AgX's linearized output into a non-sRGB canvas without encoding it, so AgX looks darker than ACES there. The Rust renderer reproduces this for parity; correcting it should be a deliberate change in both engines.
- **Physics.** Rapier replaces cannon-es. Driving is close but not identical; see [Rust simulation](rust-simulation.md) for the full list. One visible case: the VEHICLE PLAYGROUND kart's authored tuning wheelies and backflips under sustained full throttle in both engines, after about 1.0 second in Rust and about 2.3 seconds in TypeScript.
- **Normal maps.** TypeScript skips a normal map wherever its derivative tangent frame falls under a fixed 1e-7, which at ordinary texture and world scales disables normal maps entirely; Rust uses thresholds relative to the UV and position scale, so normal maps shade.
- **Shadow batching.** TypeScript draws point-light shadows in additive passes of four lights; Rust draws every shadowed light in one pass. Shadows also render on WebGL 2 in Rust, where TypeScript draws none.
- **Tools:** Joy Editor (the `joy-editor` package in `editor/`), FORM LAB (`form-lab` in `editor/tools/form-lab`), and PARTICLE LAB (`particle-lab` in `editor/tools/particle-lab`, see [its README](../editor/tools/particle-lab/README.md#rust)) run on the Rust engine beside their TypeScript; Joy Editor hosts both labs as tabs. They build on the [authoring kit](rust-authoring.md) (lab controls, script editor with the FORM and JOYFX catalogs, panel layout, file dialogs) and the Rust renderer. The TypeScript tools and their Vite development plugins stay in place beside them.

## Migration Path
1. **Games** (done): every JoyGames game has a Rust crate beside its TypeScript project (`projects/<game>/rust/`), checked against values recorded from the TypeScript game.
2. **Engine subsystems** (done): every runtime subsystem has a Rust port in its `src/` folder, with parity tests against the TypeScript reference.
3. **Tools** (done): Joy Editor, FORM LAB, and PARTICLE LAB run on the Rust renderer and runtime, so games and tools share one renderer.
4. **Publication** (done): the JoyGames site build publishes every Rust web build, beside the TypeScript build by default (`joy.joyconfig` can replace or omit it per project).
5. **Next:** switch projects to their Rust builds as each is verified, then retire each TypeScript game, tool, and subsystem once nothing uses it.
