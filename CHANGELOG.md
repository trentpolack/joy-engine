# Changelog

This changelog summarizes meaningful work on Joy Engine, Joy Editor, FORM EDITOR (formerly FORM LAB), JOYFX EDITOR (formerly PARTICLE LAB), and the engine examples. Entries are grouped by day, newest first, using America/Detroit dates. Work before 10-08-2026 happened inside [JoyGames](https://github.com/trentpolack/JoyGames); its [CHANGELOG](https://github.com/trentpolack/JoyGames/blob/develop/CHANGELOG.md) is the historical record for that period.

## 10-21-2026
### FORM v2 (Breaking)
- Added deeply immutable `const` values, including geometry and instance aliases, with editable explicit copies. Replaced `repeat` with exclusive integer `for index in start..end` loops and added ordered `foreach item in collection` iteration. Migrated engine assets, packed instances, editor recipes, completion, highlighting, and references; authored enums use multiline PascalCase entries.
- Removed FORM's immediate mode: scripts build explicit geometry values and only `output(name, geometry)` adds to the result. The pen built-ins (`color(r, g, b)`, one-argument `material`, `uv`, `normal`, `point`, `triangle`, `surface`, `instance`, positional `box`/`sphere`, `userData`, `with userData`, and `setUserData`) fail with migration messages. Added `merge`, `append`, `place`, `realize`, `color(geometry, rgba)`, `material(geometry, material | "PaletteName" | null)`, `addPoint` attribute records, `hasAttribute`, and `removeAttribute`.
- Added logic and structure to the language: booleans with `true`/`false`/`null`, `AND`/`OR`/`NOT` (conditions require booleans; `number(flag)` converts), `if`/`else if`/`else`, `match`/`case`, block-scoped `let` with `=`, `+=`, `-=`, `*=`, and `/=` assignment, `enum` declarations, text comparison, records and lists with indexing and `itemCount`, `fn` functions whose arguments are copies unless declared `ref`, and `use "path.form" as alias` modules.
- Parameters accept booleans, free text, option text, and enum members (`param kind = Kind.tower`), and the evaluator reports wrong-typed values by kind ("Expected a number; found text.").
- Custom attributes are typed (numbers, vectors, colors, text, true/false, and face materials) and realize into `GeometryData::vertex_attributes` and `triangle_attributes` (`AttributeColumn`, `vertex_record`, and `triangle_record`), replacing `GeometryData::user_data`. Named outputs are typed `NamedOutput` values whose snapshot converts to JSON on demand (`OutputValue::json`); serialized results keep the same shape.
- Procedural geometry moved from JSON records to native columns, with snapshots converted only when read. Heavy scripts evaluate two to four times faster than v1 (FRAMEBREAK's district 26 ms to 6 ms; orbital bloom 114 ms to 55 ms); small scripts cost a few milliseconds more in the interpreter.
- FORM EDITOR's script editor completes keywords, built-ins with signatures, and the names in scope at the cursor (`form_document_symbols`, the `BUILTINS` catalog, and `ScriptEditor::with_document_symbols`).
- Migrated every engine FORM asset, example, and the building structures to v2. FORM placements are isolated points that set at least one attribute, and their `scale` is FORM's standard XYZ vector.

### FORM Instances
- Renamed `.formlab` documents to `.forminst` FORM instances (`FormInstance`, version 2) that reference a `.form` script by relative path (`"form"`) or pack one (`"source"`), with overrides, materials, and variants. FORM EDITOR opens version 1 `.formlab` files as packed instances. The engine structures and form-building presets now share one `building.form`.
- `FormAssetLibrary::resolve` and `resolve_asset` take a reader for referenced scripts and modules, and their cache keys include those files. Hosted tools receive a project file reader (`ToolFileReader`, `AuthoringTool::set_file_reader`), documents carry their project path (`ToolDocument::path`), and Joy Editor saves an instance's script with it (`save_dependencies`).
- The `render` component's asset kinds are `form` and `forminst` (`component_asset_kind::FORM_INSTANCE`).

### Tool Names
- FORM LAB is now FORM EDITOR (`tools/form-editor`, package `form-editor`) and PARTICLE LAB is now JOYFX EDITOR (`tools/joyfx-editor`, package `joyfx-editor`), across packages, UI, and docs.

### Browser Loading Screen
- Web builds open on a stylized loading screen in the Joy Games site's look (navy, orange rules, Archivo Black, and hard offset shadows) that steps through download (with measured progress against the module's build-time size), compile, and warm-up, then fades out over the first presented frame. Startup failures, including the engine's failure panel, stay on the screen with a reload button.
- Added `scripts/rust/web-status.css`, which `build-web.mjs` inlines into every page alongside copies of the Archivo fonts, and which hosting development servers can serve for matching building and update screens. Titles such as `"LUNA · The Hollow Observatory"` split into a heading and subtitle.
- Browser captures detect startup failures through the loading screen's `data-state="failed"` and wait for it to clear before capturing.

### Authoring UI
- UI palettes and font choices now come from `assets/ui/theme-default.json`, with shared dimensions in `assets/ui/layout.json`. Cargo generates native/browser constants directly from those files and rejects invalid values. Removed the duplicated Rust definitions and comparison test; migrated editor, lab, and development widgets to separate `theme::colors` and `theme::layout` values and renamed `apply_authoring` to `apply_editor`.
- Simplified PARTICLE LAB to a compact single-row header matching FORM LAB and removed its standalone bottom strip. Joy Editor's Content Browser now reserves the actual height of Project Details and Documentation, with long metadata scrolling above the button instead of covering it with the status bar.
- Matched the authoring chrome more closely to the deployed JS/TS FORM LAB: heavier Archivo weights, blue panel hierarchy, full-height controls, outlined sliders and preview modes, parameter spacing, and script leading. Bundled Apache-licensed Meslo LG S for Menlo-like code typography using the shared code-size setting, restored FORM's default pane proportions without overwriting saved layouts, and kept preview controls usable in narrow panes. Web builds include the font's upstream attribution and license.
- Registered the shared authoring font families before the first UI pass, fixing the `joy-display` startup panic in Joy Editor and the labs while preserving games' default body font.
- Numeric sliders snap to hundredths by default or to an authored step, without decimal noise. Removed PARTICLE LAB's coarser step overrides while preserving exact authored values, and covered dragging and short-script focus through real input events.

### FORM Assets in Levels
- FORM parameters accept presentation metadata in `meta(...)`: `label`, `group`, `info`, `kind` (`int`, `bool`, `seed`, or `enum`), and `options`. Whole-number kinds round overrides, booleans accept JSON flags, and `joy_engine::form::parameter_control` and friends read the descriptors.
- FORM LAB and Joy Editor's level inspector show script labels, groups, and help, with toggles, option choices, and seeds with a reroll button (`toggle_control`, `choice_control`, and `seed_control` in the authoring kit).
- `FormAssetLibrary::resolve_asset` returns a `FormAsset` with the mesh and its placements: the points a script tags with user data. Placements naming an `object` template spawn level entities (`<parent>/<index>`, one level deep, at most 4,096 per asset), which Joy Editor shows and games simulate.
- `render` components take an optional FORM LAB `variant`; `AssetScene::prepare_level` builds a run-ready level from authored documents and `AssetScene::follow` presents runtime entities with the prepared assets.

### Hot Reload and Development
- Added `GameAssets` with `game_assets!` and `source_root!`: games embed text assets and, in native development builds, read them from the project folder and poll for edits (see `docs/hot-reload.md`).
- Added `DevEditors`, `DevEditorsOptions`, `TuningPanel`, and `development_file`, the standard post-processing and TUNE editors games attach, plus `ConfigField::new`.
- `HeadlessShell::new_or_skip`, `HeadlessShell::save_capture`, and `region_mean_color` replace the helpers every game's render tests carried.

### Joy Editor
- Restored **Play**: the native editor runs the selected project's game (`npm run dev`) in its own window, with Stop, and ends it when the editor closes; saved assets reach games that use `GameAssets` live.
- Added Ctrl/Cmd+O (open a project folder) and Alt+1–9 (switch tabs) to the editor, and Ctrl/Cmd+O to standalone FORM LAB, plus Ctrl/Cmd+S and Ctrl/Cmd+O to standalone PARTICLE LAB.

### Authoring Look
- The authoring theme installs the Archivo family (Archivo, Archivo SemiBold, and Archivo Black, under the SIL Open Font License) and registers semantic text styles (`display`, `title`, `caption`, and `status`); buttons and headings use the semibold face.
- Design tokens now live in `assets/theme/joy-tokens.json`, verified against the Rust constants by a test, and gained the workspace surfaces and development overlay fills. Joy Editor's private palette and the development dock's own colors now read the shared tokens, and the editor's literal type sizes follow the token scale.
- Added shared chrome to the authoring kit (`app_title`, `header_rule`, `project_marker`, `status_bar`, `list_row`, `caption`, and letter-spaced `spaced` text); Joy Editor, FORM LAB, and PARTICLE LAB draw their titles with it, PARTICLE LAB's footer is the shared status bar, and `panel_heading` uses the real fonts.

### Repository Guidance
- Engine work that absorbs consumer behavior now migrates those consumers in the same change; completion reports have their own section, agents that cannot file issues propose them in reports, and the Node spacing rules live only in the coding standards.

### Rendering
- Fixed a startup validation panic by removing the stale ACES uniform binding from the display resolve layout.

### Patch Delivery
- Hosted patch application now searches both the host delivery directory and the source repository's `.patches/`, warns about duplicates, and removes matching bundles only after success or already-applied detection. Different patches and conflicts retain every bundle.

## 10-19-2026
### Engine Organization
- Moved unit tests from `tests/unit/` to sit beside the module they test as `<file>.test.rs` (`mesh.rs` and `mesh.test.rs`), declared with a same-folder `#[path]`, so tests sort and move with their module and `tests/` holds only integration tests, fixtures, and tooling tests. See the [Rust coding standards](docs/rust-coding-standards.md#tests).

### Rendering
- Fixed display output: the resolve pass now encodes sRGB after tone mapping (grading, antialiasing, and film grain work on the encoded values), so colors decoded on input display as authored, through sRGB and non-sRGB presentation formats alike.
- Breaking: removed `FrameInputs::time_seconds` and `TriangleFrame::time_seconds`. The shell sets one presentation clock with `Renderer::set_presentation_time` (`PresentationTime`, with `fixed_rate` for tests and offline captures), which temporal antialiasing, exposure adaptation, film grain, and volumetric noise all read.
- Transparency now draws after the lighting scene effects and before temporal antialiasing, motion blur, exposure, and post-processing, and every transparent element (blended meshes, transient triangles, and `transparent_draws` such as particles) sorts back to front in one queue, so a particle behind glass draws behind it. Breaking: removed `sort_transparent_triangles`.
- Opaque geometry rasterizes once per view: a depth pre-pass (terrain first, then batches front to back; `RendererOptions::depth_prepass`) fills depth, the scene pass writes the scene-effect normal, roughness, and motion attachments beside its color instead of in two extra passes, and the sky draws last on the far plane.
- Each shadow view draws only the casters inside its cascade or point-light face, and a point light renders shadow faces only when a caster reaches it.
- Material maps and sprite and particle textures get GPU-generated mip chains, sampled trilinearly with anisotropic filtering.
- Added HDR output: on displays that can show HDR, the shell presents an extended-range surface (scRGB natively, extended sRGB in browsers with HDR canvases), follows the display's headroom, and the display resolve extends tone-mapped highlights into it while midtones and the UI stay at SDR white (`DisplayOutput`, `Renderer::set_display_output`, and `ShellOptions::high_dynamic_range`, on by default). Breaking: `EguiLayer::new` takes the display output.
- Added a main-scene render scale (`RendererOptions::render_scale`, `Renderer::set_render_scale`, and `Renderer::render_size`): the scene, its effects, and bloom render below the drawing-buffer resolution and the display resolve upscales them with a bicubic filter, while antialiasing, grain, and UI stay at full resolution.

### Meshes and Materials
- Breaking: added the `joy-render-types` crate (`joy_engine::render_types`) for GPU-free render data, and moved `DirectionalLight`, `PointLight`, `Skylight`, meshes, and materials there from `joy-core`. See [meshes and materials](docs/materials.md).
- Breaking: `Mesh` replaces `IndexedMesh3D`, `MeshAssetData`, `PbrMesh`, and `MeshChannels` with typed attributes (`DVec3` positions, `Vec3` normals, `Vec2` UVs, and `LinearColor` colors), per-triangle material slots, and named `MaterialSlot`s. `MeshDocument` (a mesh plus one material per slot) is the saved format; `dcc::parse_glb_mesh`, `create_chunk_mesh_data`, and FORM's `GeometryData::to_mesh` produce meshes, and the form-building cottage was re-baked as a document.
- Breaking: meshes no longer choose their shading. Each slot's `Material` names a shading model through `Shading` (`Layered`, `Lit`, `Unlit`, or `Custom`), mesh assets carry each slot's default `MaterialAsset`, and instances override slots (`with_material` and `with_slot_material`). Removed `Shading::Pbr`, `MeshInstance::with_shading`, and `MeshInstance::scene_roughness`.
- Added layered shading: `SurfaceLayer`s with F0/F90 and metallic reflectance, haze, Charlie sheen, coat, and thin-film iridescence lobes, composed in `Surface` trees (coat, blend, add, and weight) that collapse to one evaluated layer.
- Added a shading-model registry: built-in lit, layered, and unlit models and game models registered with `Renderer::register_shading_model` implement one WGSL contract, and the engine composes each into the lit, depth pre-pass, and masked shadow-caster passes, so a new model needs no renderer change. Every built-in model is validated for WebGPU and WebGL 2.
- Added surface debug views (`DebugView`, `Renderer::set_debug_view`): base color, diffuse albedo, specular color, roughness, metallic, normals, emissive, occlusion, lighting only, shading model, shadow cascades, and texture coordinates.
- Breaking: `ShadedShape`, `ShadedTriangle`, and `add_shaded_shape` replace `ShadedMesh3D`, `ShadedTriangle3D`, and `add_mesh` for arcade geometry, and `compile_form_mesh`, `form_mesh_asset`, and `FormAssetLibrary::resolve` take a `ShadingModel`.

### Display and Scene Effects
- Breaking: replaced the luma.gl-derived display chain with engine-owned passes: a bloom pyramid (soft threshold with firefly suppression, 13-tap downsample, and tent upsample) whose depth follows `bloomRadius`, then one resolve for exposure, bloom, the display transform, vignette, HDR headroom, and dithering. Removed brightness, contrast, saturation, vibrance, blur, post antialiasing, film grain, anamorphic bloom, lens flare, and vignette blur from `PostProcessSettings`. See [postprocessing](docs/postprocessing.md).
- Breaking: display transforms are filmic (the new default, with a parametric toe, linear section, and shoulder), AgX with default and punchy looks, ACES 2.0 (the full Output Transform for the display's peak luminance, ported from OpenColorIO and replacing the ACES 1.x curve), and none.
- Breaking: replaced the luma.gl scene effects with temporal antialiasing, motion blur, adaptive exposure, and volumetric fog and lighting, and removed SSAO, GTAO, SSGI, screen-space reflections, outlines, height fog, clustered volumetric lighting, and depth-aware blur. Fog is lit and shadowed by the key light, point lights, and the skylight, and fogs transparent surfaces too. See [scene effects](docs/scene-effects.md).
- Scene effects now run on WebGL 2 where float render targets exist (adaptive exposure meters without compute there); only volumetric fog needs compute shaders, and `Renderer::scene_effects_capabilities` and the preview controls report why a requested effect is skipped. Breaking: removed `SceneEffectsOptions::color_format`, `Renderer::scene_volume_stats`, and the `Contact` preset (now `Cinematic`).
- Replaced the luma.gl-derived image-based lighting: environments are captured into a radiance cube without missing small HDR suns, specular levels are prefiltered with GGX filtered importance sampling (noise-free at 64 samples), diffuse light is L2 spherical-harmonic irradiance computed from the source pixels instead of a Monte Carlo cube, and the renderer builds one split-sum BRDF table with the direct lights' height-correlated Smith visibility. Breaking: `EnvironmentMapOptions` dropped `irradiance_size` and `brdf_lut_size`, and its default specular cube grew from 64 to 128 texels per face. See [meshes and materials](docs/materials.md#image-lighting).
- Web builds' `THIRD-PARTY-NOTICES.txt` now includes OpenColorIO's license for the ACES 2.0 port.

### Profiling and Console
- Added frame profiling (`Renderer::set_profiling` and `Renderer::frame_profile`): CPU recording time per frame phase, and GPU time where the device has timestamp queries (WebGPU and native; `GpuCapabilities::timestamps`), read back without stalling. The STATS panel shows it in place of "Not collected".
- Added an Unreal-style development console on the backquote key with typed console variables (`ConsoleVariables`) games can register, `help`, `find`, `reset`, and `stat unit` / `stat gpu` overlays, history, and completion. Engine `r.` variables drive render scale, debug views, profiling, the sky, shadows, and each scene effect, mirroring the renderer's actual state. Breaking: backquote opens the console instead of the STATS panel, which its dock launcher still opens. See [console and profiling](docs/console-and-profiling.md).
- The STATS panel gained a Rendering section with a debug view selector and the profiling switch.

### Lighting and Shadows
- Added deferred lighting on every backend but WebGL 2 (`RendererOptions::deferred`, `Renderer::set_deferred`, and `r.Deferred`, on by default): opaque surfaces write emission, indirect light, and a packed G-buffer in one pass, and one stencil-masked, full-screen pass per shading model adds direct light through the model's own response, so a pixel pays for its lights once however surfaces overlap. Shading models opt in with `materialEncode` and `materialDecode`; the lit and layered models and terrain do, and layered materials whose features exceed the G-buffer (thin film, colored F90, tinted coats, or two optional lobes) shade forward per material. WebGL 2 and debug views shade forward. See [lighting and shadows](docs/lighting.md).
- Point lights are binned into light clusters each frame, so every lit pass (forward and deferred surfaces, terrain, and volumetric fog) reads only the lights that reach each surface, and `MAX_POINT_LIGHTS` rose from 32 to 1024. `RendererDiagnostics` reports cluster references and drops, the lighting path, lighting passes, and forward-shaded batches; the light complexity debug view shows how many lights reach each surface, and frame profiles time the G-buffer and deferred lighting phases.
- Rewrote shadows as engine code and removed the last luma.gl-derived module (and its license from web builds' notices). Point lights render their six faces into importance-sized tiles of one depth atlas in a single pass, so distant lights cost little and crowded frames shrink the least important lights' faces before dropping their shadows (`ShadowOptions::max_point_lights` and `ShadowQuality::point_atlas_size`). Cascades reach toward the light only as far as the casters above them, and receivers use percentage-closer soft shadows sized physically from the light's angular or source radius, receiver-plane depth bias, texel-scaled normal offsets, and per-pixel rotated Vogel disks.
- Breaking: the scene depth buffer is `Depth24PlusStencil8`, `FramePhase` gained `GBuffer` and `Lighting`, `DebugView` gained `LightComplexity`, and `GpuCapabilities` gained `deferred`.

### Lights
- Frames may supply any number of point lights: the renderer skips invalid lights and lights that cannot reach the view, then draws the 32 that light it most (`RendererDiagnostics::point_lights` and `point_lights_omitted`), instead of failing the frame above 32.
- Breaking: `DirectionalLight`, `PointLight`, and `Skylight` are shared `joy-core` records. `CelestialLight` became `DirectionalLight` (`FrameInputs::directional_light` supplies a key light such as FORM LAB's studio light), the environment's sun and moon are `AtmosphereLight`s (a `DirectionalLight` plus its `CelestialBody`), and `EnvironmentLighting::skylight` replaces `skylight_color` and `skylight_intensity` with a sky color and a ground-bounce color.
- Particle lights are `PointLight`s ranked with level lights, and emitters can opt into shadows with `light.castsShadow` (PARTICLE LAB's "Cast Shadows").

### Terrain
- Breaking: terrain draws only as `RenderScene` terrain instances. Removed `TerrainRenderer`, `TerrainFrame`, `RenderTarget::draw_terrain`, and terrain GPU timing; `Renderer::terrain_stats` reports an instance's statistics, and the terrain example draws through the scene renderer.

### Tests
- Recorded fixtures are now golden recordings re-recorded from Rust: `joy-test-support`'s `GoldenFile` checks a test's rebuilt recording with a numeric tolerance, and `JOY_UPDATE_FIXTURES=1 cargo test -p <package>` re-records it, rewriting only values that changed and keeping each file's layout. Games reach it as `joy_engine::testing` through the facade's `test-support` feature in their dev-dependencies. `assert_matches_recording` was replaced by `GoldenFile`.
- Tests, fixtures, and source comments no longer present the retired TypeScript engine as the reference; comments state the real constraint (seeded determinism, golden recordings, or the authored document format) instead.

### Physics and Simulation
- Breaking: vehicles are physics bodies. `PhysicsWorld::create_vehicle` returns a `BodyHandle` (`VehicleHandle` was removed), vehicle queries take the chassis body, and `joy-gameplay`'s world no longer keeps a separate vehicle map (`LevelRuntime::vehicle` folded into `body`).
- Fixed-step physics keeps each body's previous pose: `PhysicsWorld::interpolation`, `interpolated_body_transform`, and `interpolated_wheel_transforms` blend presentation between steps, and `World::presented_entities` hands those transforms to `LevelScene`.

## 10-18-2026
### Engine Organization
- Breaking: renamed the shell contract from `Game` to `App` (`app.rs`, with `AppResult`, `AppError`, and `ShellError::App`), since games, tools, and examples all implement it, and moved `FixedStepLoop` from `joy-app` into `joy-core` (still in the prelude).
- Replaced `architecture/layers.json` with layer names in each crate's `[package.metadata.joy]` and the layer order and backend ownership in `[workspace.metadata.joy]`; `npm run check` reads both through `cargo metadata`.
- Moved FORM LAB and PARTICLE LAB from `editor/tools/` to a top-level `tools/` directory, and gave Joy Editor and FORM LAB the standard Cargo source layout (all modules under `src/`, with no `site/src/` or `#[path]` source modules). The terrain example's height data moved to `examples/terrain/assets/`.
- Every package now keeps its tests in one `tests/` folder: a single Cargo-discovered `integration/` binary, `unit/` files that mirror the source modules they test (moved out of 79 inline `#[cfg(test)]` modules), `fixtures/`, and `tooling/` for Node and Python tests. See the [Rust coding standards](docs/rust-coding-standards.md#tests).

### Math and Determinism
- Breaking: `joy_engine::math` (`joy_core::math`) is the engine's one math path, re-exporting glam's `f32` types and `DVec2`/`DVec3` for `f64` simulation; only `joy-core` depends on glam, and the facade no longer re-exports `glam`. Retired `js_math`, `Vector2`, `Vector3`, `Velocity2`, `MovingBody`, `AffineTransform3D`, `GeometryBounds`, and the other JavaScript-compatibility helpers.
- `math::deterministic` provides portable `f64` transcendental functions, and a workspace `clippy.toml` rejects the standard `f64` versions, so simulation matches between native and WebAssembly builds. Particle scripts, FORM, and the editors now follow Rust semantics (character columns and limits, authored key order, typed JSON, and Rust rounding) instead of emulating JavaScript.

### Colors
- Breaking: replaced `core::Color`, `Rgb`, `Rgba`, `parse_hex_color`, and `with_alpha` with `Color` (sRGB-encoded) and `LinearColor` (linear light, HDR-capable), both `f32` RGBA with straight alpha and exact sRGB conversions.
- Fixed RGB clamping at one, which clipped glow and shifted hues: FORM `color()`, color attributes and parameters, FORM material emission, particle color parameters, and 2D mesh shading now keep HDR values (alpha stays 0–1), and FORM LAB exports HDR emission with `KHR_materials_emissive_strength`.

### Post-Processing Volumes
- Breaking: post-processing is now a `postProcess` entity component instead of a level-wide `postProcessing` profile, and levels that still have the profile fail to parse with a pointer to the component. Unbounded volumes apply everywhere; box and sphere volumes apply inside the entity's shape with a `blendDistance` fade, ordered by `priority` and scaled by `weight`. `PostProcessVolumes` in `joy-scene` resolves them at a view point each frame.
- Moved post-processing settings from `joy-core` to `joy-renderer::post_process` (presets and versioned profiles were retired) and the presentation components (`render`, `light`, `effect`, and `postProcess`) to `joy-scene`. The development post-processing editor and Joy Editor's level viewport now edit and preview volumes.

## 10-17-2026
### Engine Crates
- Split the first engine systems out of the single `joy-engine` crate into their own workspace crates under `crates/`: `joy-core` (foundation data, now also holding `scripting`), `joy-physics`, `joy-gameplay`, `joy-form`, `joy-dcc-gltf`, and `joy-project`. `joy-engine` re-exports each under its existing module name, so `joy_engine::physics`, the prelude, and every consumer path are unchanged; `core::format::format_bound` is now public, and `scripting` is also reachable as `core::scripting`.
- Moved each crate's headless integration tests and fixtures into the crate, so `cargo test -p joy-gameplay` (and the physics, FORM, glTF, project, and core suites) build without wgpu, winit, or egui. The `joy-form-evaluate` CLI now lives in `joy-form`; FORM LAB's parity harness builds it with `--package joy-form`.
- Moved shared package fields, dependency versions and features, and lints into `[workspace.*]` tables that the facade, engine crates, editor, and labs inherit.
- `architecture/layers.json` now assigns crates as well as the remaining `src/` modules to layers; `npm run check` verifies crate dependencies through `cargo metadata`, and keeps tools and examples on `joy-engine` instead of the system crates. The repository audit checks the new crate entries.
- Removed the stale `dist-rust/` build outputs that were committed with the TypeScript retirement; browser builds write to the ignored `dist/`.
- Moved the environment, particles, and renderer into `joy-environment`, `joy-fx` (renderer-free `.joyfx` documents, scripts, and simulation), and `joy-renderer`, and added the `joy-scene` bridge for `LevelScene`, `AssetScene`, the FORM mesh adapters, and environment lighting. The renderer now depends only on `joy-core` and `joy-fx`; `joy_engine::rendering` and `joy_engine::particles` combine the crates, so their paths are unchanged.
- Breaking: sky presentation settings (`AtmosphereOptions`, `CloudOptions`, `LightShaftOptions`, and `AmbientOcclusionOptions`) moved from `EnvironmentOptions` into the renderer's `SkyOptions` (same defaults, names, ranges, and messages; validate with `SkyOptions::validate`), and `environment_lighting(&frame, &sky)` replaces `EnvironmentLighting::from(&frame)` with identical results.
- Breaking: `compile_form_mesh` and `FormAssetLibrary::resolve` return `FormMeshError` (the FORM diagnostic, or a wrapped `RenderError`) instead of `RenderError::Form`, with the same messages.
- Split the platform layer into `joy-app` (the windowed and headless shells, frame phases, and clocks), `joy-input`, `joy-audio`, `joy-storage`, `joy-ui` (the egui layer, theme, HUD typography, and display metrics), `joy-devtools` (the development dock, config panels, live tuning, and effect editors), and `joy-authoring` (the authoring kit and `AuthoringTool`), each with its browser backend; the facade's `browser` module dissolved into them. `joy_engine::runtime` and `joy_engine::development` combine the crates with unchanged paths, so the facade's `src/` now only re-exports engine crates.

### Development Workflow
- Aligned CI and local validation with nightly rustfmt for vertical imports, formatted the Rust workspace, and retained stable Rust for Clippy and builds. Resolved fixed-size chunk iteration lints in SDF mesh validation, the collision example, and its tests without changing mesh handling.

### Repository Audits
- Added a standalone repository audit, shared read-only checks, and an independent fixture suite. Reports identify maintenance issues and review candidates without requiring a hosting repository.

## 10-16-2026
### SDF Collisions
- Added signed distance field colliders to `physics` as a prototype: `SdfShape` (analytical primitives, or any triangle mesh baked into a voxel distance grid that tolerates open or overlapping procedural meshes) and `PhysicsWorld::create_sdf_body`. Contacts follow Giles and Andrews' caching and importance-sampling method (Pacific Graphics 2026) through a chained narrow-phase dispatcher, so worlds without SDF bodies simulate exactly as before, and SDF simulations stay deterministic natively and in WebAssembly. See [SDF colliders](docs/rust-simulation.md#sdf-colliders).
- Added the SDF COLLISIONS example: FORM props piling up in an SDF bin beside the same props as bounding boxes, with live costs and a `--bench`/`?bench=1` benchmark. Web builds' `THIRD-PARTY-NOTICES.txt` now also carries the reference implementation's MIT license.

### Rust Runtime Default
- Retired the deprecated engine, editor, lab, and example runtime implementation and its renderer dependencies. Rust is the shared native/browser implementation; canonical npm scripts wrap Cargo and browser builds.
- Replaced current runtime and contribution documentation with Rust guidance and generated rustdoc, retaining useful Node tooling, asset schemas, capture orchestration, and recorded compatibility fixtures.

### Continuous Integration
- Added locked Rust formatting, Clippy, native workspace tests, and WebAssembly checks for pull requests and protected branches, with pinned GitHub Actions and shared build caching.

### Cloud Patch Delivery
- Added standalone cloud patch fetch/apply commands and delivery guidance, with engine-owned tooling shared by hosting workspaces. Both entry points preserve local staging, reconcile in disposable repositories, and remove all four bundle files only after successful or already-applied handling.

### Fixes
- Enabled the GLSL writer explicitly for native shader tests, so the standalone Rust workspace's Clippy and test commands compile on macOS without relying on a host OpenGL backend.
- Added `runtime::SaveSlot`, which refuses ordinary writes after a failed read or a rejected save until the game explicitly replaces it, so an autosave can no longer overwrite progress that merely failed to load (a valid but unreadable save used to be replaced by a fresh one). See [Rust storage](docs/rust-storage.md#saves-saveslot).
- Browser builds now pass Shift, Ctrl, Alt, and Command to egui (with Cmd as the shortcut key on Apple platforms), so Shift-panning, Shift-selection, and modifier shortcuts work in browsers as they do natively.
- A touch that starts on a UI widget now stays with the UI until it ends, so dragging a slider or scrolling a panel can no longer become a gameplay touch underneath.
- The declared minimum Rust version is now 1.95 (egui 0.36 requires it; 1.90 builds failed), and the code passes Rust 1.99's Clippy, which flags `chunks_exact` with a constant size in favor of `as_chunks`.

### Rust Engine
- Added pointer lock to the Rust shell: `Frame::set_pointer_lock` captures the cursor (winit's locked grab natively, falling back to confined; `requestPointerLock` in browsers), `InputManager::pointer_motion` reports raw relative mouse movement per frame while locked, and `InputManager::pointer_locked` reports whether the lock is held, so a game can pause when the browser or window releases it.
- Added `InputManager::touch_points`, every active touch with its id, so on-screen sticks and look pads follow the finger that started on them.
- Added JavaScript `Math.tan`, `Math.acos`, and `Math.asin` to `core::js_math`.
- Added `AudioBank::set_master_volume`, which scales sounds as they start, for game volume settings; VECTORS TO VICTORY's synthesized sound board is its first consumer.
- Fixed PBR shading of smooth-shaded silhouettes: normals now flip only on back faces, as in the TypeScript pass, instead of always turning toward the eye.

### Rust Tools
- Added the authoring kit to the Rust engine's `development` module for FORM LAB, PARTICLE LAB, and Joy Editor: lab controls (exact number fields with scrubbing and Shift/Alt steps, bounded scalars, XYZ/RGBA groups, a linear-RGBA color picker, defaults and resets, rejected-draft tracking), property undo, a script editor with catalog highlighting, completion, hover help, and diagnostics, resizable panes, viewport gestures, native and browser file dialogs (rfd natively), and the Joy design tokens as an egui theme. See [Rust authoring kit](docs/rust-authoring.md).
- Ported PARTICLE LAB to the Rust engine as the `particle-lab` package beside its TypeScript, natively and in browsers, drawing only through the engine renderer and hostable in Joy Editor. Added `RegionCamera`, particle texture source encoding, runtime particle profiling toggles, and `HeadlessShell::step_with_ui` for driving tool interfaces in tests.
- Ported FORM LAB to the Rust engine as the `form-lab` package beside its TypeScript, natively and in browsers: `.form` and `.formlab` documents with variants, the examples library, parameter, material, lighting, and output inspectors, and GLB (byte-identical to the TypeScript exporter), OBJ, PLY, JSON, and MaterialX exports, with the preview drawn by the engine renderer and hostable in Joy Editor. Added JPEG and WebP material maps, `FormTextureCache`, running-renderer sky and shadow setters, and FORM value validators for tools.
- Ported Joy Editor to the Rust engine as the `joy-editor` package in `editor/`, a native application that replaces the Electron host: project discovery, the content browser, tabs with revision-checked saves, external-edit handling, and recovery copies, session restore, the schema-driven level editor with a move gizmo, and FORM LAB and PARTICLE LAB hosted as tabs through `development::AuthoringTool`. Browser builds open bundled examples read-only. Added `Game::close_requested` so a game or tool can keep its window open for unsaved work.
- Added `rust:web` scripts to the published examples, FORM LAB, PARTICLE LAB, and Joy Editor, so a hosting site builds their Rust pages the same way it builds games (`npm run rust:web -- --out <folder>`); `dist-rust/` staging folders are ignored. `build-web.mjs --dev` keeps a script's title and canvas but builds with the fast dev profile, for development servers.

### Rust Examples
- Ported SCENE EFFECTS, FORM BUILDING, and FORM DESTRUCTION to the Rust engine (`cargo run --example <name>`, `npm run rust:web -- --example <name>`), each beside its TypeScript version with headless tests and captures in `test/rust/`. The building draws its FORM materials through the PBR pass, and the shadow-material fixture became a Rust render test.
- Added small engine hooks they needed: `ShiftedCamera` (lens shift beside side panels), `Stopwatch`, `save_text_file` exports (Blob downloads in browsers), `MeshAsset::from_asset_data`, and per-quality tuning for `SceneEffectsControls`. HUD taps on egui widgets no longer also reach gameplay.

## 10-15-2026
### Rust Engine
- Added the `joy-engine` Rust crate (`Cargo.toml` at the root, `src/lib.rs`), with Rust modules beside the TypeScript in the same `src/` folders and dependency layers, so games and tools run natively and in browsers (WebGPU with WebGL 2 fallback) from one codebase. The TypeScript engine keeps running existing games and tools until they move over.
- Added the Rust `rendering` module, a wgpu renderer matching the TypeScript frame: Lit, PBR, and Unlit material passes, filtered directional shadows on every backend, point lights, the procedural atmosphere, and the full display-processing chain from the same post-processing profiles. It is the single renderer for games, FORM content (`FormAssetLibrary`), level presentation (`LevelScene`), and tools; FORM stays renderer-free.
- Added Rust `environment`, `physics` (rapier rigid bodies and raycast vehicles), `gameplay` (entities, controllers, levels, and grid navigation), and `form` modules, checked against values recorded from the TypeScript engine, and `runtime`, a `GameShell`-style frame-phase runner with input, multi-touch, egui HUDs, a development overlay, and headless runs.
- Added `scripts/rust/build-web.mjs` (`npm run rust:web`) for static browser builds, a `form-viewer` example, Rust layer checks in `npm run check` that reuse the TypeScript layer roots, and the [Rust engine guide](docs/rust.md) with parity notes and the migration path.

### Workspace Configuration
- Renamed FORM LAB and PARTICLE LAB's published site category from `app` to `tool` to match the hosting platform's Joy Engine Tools category.

### Game Shell
- Added `GameShell` as the shared browser composition owner for configuration, renderer and input resources, development diagnostics, ordered frame phases, failure handling, and idempotent page lifecycle cleanup; FRAMEBREAK is the first game integration.
- Extended the `.joyconfig` schema with project production availability settings for hosting-workspace builds, captures, and deployment.

### Engine Rendering Backend Wrappers
- Removed raw luma.gl re-exports from `joy-engine`: the `@luma.gl/effects` pass wildcard, `GLSLShaderAssembler`/`WGSLShaderAssembler`, and the `ShaderPass`/`ShaderPassPipeline` types. No game, example, or editor imported them; this is breaking for direct importers, who should select effects through post-processing profiles and `SceneEffectsOptions`.
- Added `GpuDevice` for the device returned by `createGpuCanvasDevice`, and `createEngineDependenciesPlugin()` in `joy-engine/development/vite` so Vite configs no longer list `@luma.gl/*` packages.
- `npm run check` now rejects luma.gl imports outside the rendering layer and any `@luma.gl/` reference in the editor or examples.
- Moved `PRIMITIVE_SHAPE` into the foundation layer, retiring three recorded layer violations.

### Components
- Added `ComponentSchemaCatalog` with built-in schemas for `render`, `light`, `effect`, `physics`, and `vehicle`: typed fields, units, bounds, conditional fields, presets, and advisory validation. Projects can register their own schemas.
- Named runtime defaults -- `PHYSICS_BODY_DEFAULTS`, `PHYSICS_VEHICLE_DEFAULTS`, and `PHYSICS_WHEEL_DEFAULTS` as well as render, light, and effect component defaults -- so schemas and runtimes share one source; values are unchanged.

### Joy Editor
- Made the level inspector schema-driven: the add-component menu lists catalog presets by category, fields use schema labels, units, bounds, choices, and asset kinds, omitted fields can be added at their runtime defaults, fields a shape ignores are dimmed, and issues such as a vehicle without physics appear inline without blocking edits.

## 10-14-2026
### Runtime Architecture
- Added a headless `World` owner for deterministic entity and physics updates, with rendering left behind an application-owned integration boundary.
- Added enforced source dependency layers with an explicit, shrinking allowlist for existing migration violations.
- Migrated `LevelRuntime` onto the headless world composition while preserving its authored update and physics synchronization order.

### Terrain Rendering
- Migrated resident terrain into scene ownership with hierarchical transforms and shared indexed GPU geometry across color, shadow, motion, and surface passes.
- Kept terrain heightfields and LOD selections GPU-resident while sharing scene lighting, shadow maps, depth, normal/roughness, and velocity attachments.

### Scene Rendering
- Added a deeply immutable `FrameView` shared by scene lighting, atmosphere, effects, and shadows, with separate pass contexts for light cameras and render targets. The low-level `SceneLightFrame` now takes `{view, casters}`; direct callers preserve renderer shadow settings with `renderer.createFrameView`.
- Applied the canonical rendering conventions across frame snapshots, scene orchestration, effects, surfaces, and shadows, with explicit ownership, documented contracts, and cohesive preparation stages while preserving submission order and temporal history behavior.

### Scene Frame Readability
- Reorganized camera matrix, jitter, queue planning, queue assembly, terrain asset validation, terrain selection, timing, and rendering code around named stages and fully documented ownership contracts, preserving rendering behavior and typed-array queue performance.

### PARTICLE LAB
- Framed each opened particle document to its simulated world-space bounds, so game-scale effects remain readable while reset restores that document-specific view.

### FORM LAB
- Kept all four DESIGN MODE frame edges visible above editor panels without blocking selection or changing the layout.
- Preserved unfinished PARTICLE LAB scripts in browser recovery, and kept FORM LAB exports paired with the preview that successfully acquired GPU resources.
- Moved twelve full examples out of TypeScript into `assets/examples/*.form`; recursively discovered FORM and FORMLAB files from `assets/examples` and `assets/form`, grouped them by folder, and preserved document resources when opening independent working copies.
- Restored header hover feedback and orange DESIGN styling with Escape cleanup, moved View beside Tools, and hid file destination controls when no native picker is available.
- Aligned the relocated tool menu to open to the right, keeping it inside narrower viewports.
- Simplified the header and removed the footer; added save/export dialogs with direct `.form` source saving, explicit `.formlab` document preservation, and native location selection with a browser download fallback.
- Added persistent Script, Preview, and Inspector visibility through an always-reachable View menu, preserving hidden panel state and keeping at least one panel visible.
- Added one LevelRuntime spawn path so initial and dynamically created entities receive matching physics and vehicle setup with rollback.

## 10-13-2026
### Gameplay navigation
- Added `searchGrid` and `traceGridRoute`: deterministic weighted reachability and cheapest-route tracing over caller-defined grid cells, with stable equal-cost tie-breaking and a bounded cell limit.

### Lab Scripting
- Added editor-neutral FORM and particle language catalogs plus shared CodeMirror completion and hover tooling for FORM LAB, PARTICLE LAB, and future scripting surfaces.

### Scene frame assembly
- Split scene-effect attachment preparation into documented motion, surface, and lighting stages while preserving update order and resource cleanup.
- Replaced flat mesh-cache signatures with named, copied vectors, preserving cache reuse and detection of in-place transform, tint, and opacity edits.
- Documented `PreparedMesh` ownership and packed layouts, with readonly fixed-length signature and affine-transform contracts composed from core `Vector3` and `Color` types.
- Reworked `prepareSceneFrame` into a two-pass assembly that sizes every queue first and copies cached `Float32Array` vertices with `TypedArray.set`. Opaque and shadow-caster queues are now `Float32Array`s; submission order, culling, roughness ranges, and transparent sorting are unchanged. In Node, CPU preparation for 185–331 instances (26,000–43,000 triangles) fell from roughly 40–70 ms to 4–7 ms per frame.

## 10-12-2026
### LAB Editing Controls
- Unified FORM LAB and PARTICLE LAB with compact scalar, vector, and linear RGBA controls, native color selection, readable modified/reset states, and borderless idle slider surfaces with keyboard focus.
- Preserved exact values, alpha, bounds, focused drafts, and serialization; added undoable FORM material-parameter tuning and color-picker regressions without changing rendering or shaders.

### Scene rendering
- Corrected WebGPU flat-lit receiver normals so shadow bias moves out of visible surfaces instead of causing self-shadowing; extended the shadow browser regression to verify illuminated and occluded colored receivers.

### Engine examples
- Added FORM Destruction's integrity heatmap and remaining-hit readout from committed chunk health, increased wall resistance and gravity, and kept released chunks until reset without expiry or eviction.
- Added FORM Destruction's large solid ground and rigid-body debris collision, with bounded, toggleable impact-point, ray, and damage-radius debug drawing.
- Added FORM Destruction as an independent example with surface-projected click impacts, cumulative damage, orbit/zoom inspection, bounded debris, and reset. Removed the game-specific movement probe and reticle.
- Opted Terrain, Scene Effects, FORM Building, and FORM Destruction into the hosting platform’s example directory and production preview workflow.

### Runtime destruction and physics contracts
- Added opt-in ground support with bounded cantilevers and atomic release of unsupported healthy chunks; FORM Destruction now shows support paths and collides falling chunks against surviving structure.
- Exposed copied committed chunk damage state so consumers can visualize partial damage without duplicating simulation health.
- Added stable chunk destruction under `src/physics/destruction` with queued simulation-boundary damage, reset, closed geometry generation, raycasts, player-volume collision queries, and behavior tests. Kept destruction solver-independent and preserved its public exports.
- Defined solver-neutral CPU/GPU physics residency and ordered mutation batches for contact invalidation and acceleration-structure updates.

## 10-11-2026

### Runtime diagnostics
- Clarified that GPU resource lifetime totals are monotonic and include transient WebGPU uploads, directing leak investigations to active handles and tracked allocation bytes.

### Joy Editor design system
- Fixed DESIGN opening an empty inspector, moved its launcher into the workspace and lab headers, and cleared inspection state on close or Escape.
- Added shared design tokens and control classes across Joy Editor, FORM LAB, and PARTICLE LAB, with a development-only inspector, token viewer, comparison view, and CSS persistence endpoint.

### FORM LAB workbench
- Compacted standalone chrome, exposed existing Recipes beside the script reference, added numeric default/modified feedback and undoable per-parameter resets, clarified browser recovery/document downloads versus baked exports, and kept complete inspectors reachable at narrow standalone and hosted widths. Preserved PBR preview and shadow rendering.

### FORM geometry
- Changed sphere primitives to generate smooth radial normals across UV seams and poles by default. Boxes retain flat shading, and sphere generation preserves the surrounding authored normal state.

### Repository
- Trimmed ignore rules to the engine’s npm, TypeScript/Vite, Electron, browser validation, and local development files.

## 10-09-2026
### Light shadows and FORM LAB materials
- Added luma.gl WebGPU directional and point-light shadows, enabled on lights by default, with configurable quality/bias, camera-independent caster queues, masked PBR casting, transparent light accumulation, and shadow-work diagnostics. Batches preserve the existing 32-point-light limit; WebGL 2 continues unshadowed illumination ([#53](https://github.com/trentpolack/joy-engine/issues/53)).
- Added an explicitly owned PBR environment adapter, portable irradiance/reflection shading, and FORM LAB studio reflections, lighting controls, and a Material studio example. Retained independent material maps, authored exports, and atomic mesh/environment replacement. Restored hoisted font serving in the standalone FORM LAB dev command.

### FORM building asset example
- Added a FORM-authored building shell with three seeded presets, editable runtime parameter snapshots, baked mesh/GLB assets, and a material-group presentation adapter. Documented compiler boundaries, material rendering limits, focused tests, and local browser evidence.

## 10-08-2026
### Runtime diagnostics
- Clarified the live development launcher as `STATS | <fps>` with stable numeric spacing so cadence changes remain easy to scan.
- Gave the shared runtime dock wider buttons and more horizontal padding so the Stats readout has room; compact layouts hide its divider alongside the FPS value.

### Joy Editor development server
- Made Joy Editor and its labs discover their active npm workspace for Vite file serving, restoring hoisted dependencies when the editor is hosted by a larger repository.

### Repository
- Clarified the agent workflow for inclusion in a larger workspace: complete file edits and validation together, leave submodule Git state to the user, and keep agent instructions independent of private hosting repositories.
- Established this repository from the Joy Engine and Joy Editor history in JoyGames (`joy-engine/` became the root, `joy-editor/` became `editor/`, the engine guide moved to `docs/`). Joy Engine 0.12.0 and Joy Editor 0.6.0 are the starting versions.
- Made the root package the npm workspace root for Joy Editor, FORM LAB, PARTICLE LAB and the examples, with its own lockfile, `.gitignore`, `AGENTS.md` house rules and coding standards. JoyGames consumes the repository as a git submodule at `joy-engine/`.

## 10-07-2026
### Engine and editor repository split preparation
- Made Joy Engine (0.12.0) a self-contained package with its own TypeScript base, unit suite, `?raw` Node loader, boundary check, API generator, guide (`joy-engine/docs`) and `joy-check-javascript` bin; it now installs, checks and tests from an isolated copy.
- Moved the CodeMirror script editor, preview fullscreen and Electron lab host from the engine into Joy Editor (0.6.0), along with the editor production-availability policy. Joy Editor now locates its hosting workspace and projects directory instead of assuming the JoyGames layout.
- Exported the config tuning, post-processing persistence and favicon Vite plugins from `joy-engine/development/vite`, sorted tests by owning package, fixed `npm run docs:api`, pinned the root engine dependency to `file:./joy-engine`, moved wrangler to devDependencies, and removed the legacy editor desktop stack.
- Documented the extraction and cut-over steps in [repository split](docs/development/repository-split.md).

### Joy Engine world objects and ECS
- Established `Entity` as the base world-object contract and added registries for data-driven runtime components, deterministic component queries and world-owned systems with explicit rollback and cleanup.
- Documented the hybrid actor/ECS migration path so existing entity subclasses and portable level component data can coexist with composable behavior.

### Joy Editor level viewport
- Restored the level viewport's scene-effects render error boundary, adaptive grid ownership, and per-frame scene submission independently of grid-spacing display updates.

## 10-02-2026
### Shared development widgets and monitoring
- Docked runtime STATS with CONFIG and JOY-ENGINE in one responsive lower-right row, coordinated panel expansion and Escape focus, and shared a development-only layout buffer with gameplay HUDs and touch controls.
- Unified runtime stats, configuration/tuning, postprocessing and example debug/help panels with near-black surfaces, Joy orange outlines and consistent hover/focus states while preserving gameplay palettes and controls.
- Added expandable, cadence-limited luma allocation/resource details with explicit shared-device scope and unavailable states; frame cadence remains distinct from CPU/GPU execution time and resets after background/resume.
- Made Particle Lab per-emitter profiling opt-in, disabled monitoring clocks and UI work when monitoring is off, exposed CPU submission/sort and vertex payload details, and replaced the speculative draw estimate with scoped scene geometry batch counts.

### Joy Editor content and lab workflows
- Shared cached, unlit line grids across Level, FORM and Particle previews with adaptive world-unit spacing, major divisions and distinct axes; added Level grid visibility and kept Particle Lab's light receiver separate and default off.
- Unified semantic theme roles and script surfaces, improved native numeric arrows, and simplified the Content Browser with multiselect filters, keyboard-accessible actions and explicit double-click opening.
- Added cancellable, self-contained emitter insertion presets, removed the generated JSON preview UI, and made lab maximization retain editing panels while restoring surrounding workspace state.
- Restored single-click file opening, clarified New FORM LAB documents versus imported source, and polished content actions, project help, preview state and FORM controls; PLY is hidden from the export menu.

### Joy Editor preview lifecycle
- Cancelled queued and in-flight automatic particle applications when Live is disabled, preserving explicit Apply and pending edits; added deterministic browser regression coverage.

### Engine and editor TypeScript migration
Migrated Joy Engine runtime implementations and public contracts to native TypeScript across rendering, physics, environment simulation, gameplay, FORM compilation, particles, input, configuration and editor services. Converted Joy Editor workspace and level authoring plus FORM/PARTICLE LAB browser modules and compiler workers. Updated package exports, all consumers, source links and API generation while preserving runtime behavior and retaining JavaScript for Node tooling and Electron launchers.

### Joy Engine terrain
Aligned the terrain example UI with the Joy orange/blue palette, square panels and controls, branded typography, and clear interaction states.
Moved the terrain demonstration into `joy-engine/examples/terrain/` with explicit command-line launch/build/capture scripts, outside the main site and automatic releases. Added camera-relative fly movement with cursor-hidden right-button look, left-drag orbit, scroll zoom and Command/Alt panning.
Documented terrain resource ownership, CPU/GPU LOD selection, shared-edge morphing, buffer layouts and matching shader logic without changing rendering behavior.

## 10-01-2026
### Joy Engine / Terrain Lab
Added an isolated Dragon Pit landscape experiment with offline EXR-to-uint16 export, resident error hierarchy, continuous shared-edge CLOD, indexed grids, WebGPU compute/indirect selection and a WebGL 2 CPU fallback. The demo documents explicit scales, stable gameplay sampling and renderer ownership; vehicle physics, road authoring and atmosphere integration remain separate.

### Joy Engine environment systems
- Added a deterministic environment simulation for solar time, sun/moon atmosphere lighting, skylight and gusting wind, with validated atmosphere, fog, cloud, light-shaft and ambient-occlusion policy.
- Added a portable environment uniform layout and standard PBR lighting adapter so future WebGPU and WebGL 2 passes share one synchronized world-state contract.
- Added the first portable atmosphere renderer for WebGPU and WebGL 2, including approximate Rayleigh/Mie scattering, sun and moon discs, wind-driven procedural clouds, and a primary-light scattering halo.

### Joy Editor workflow v2
- Corrected particle browser state/palette checks and static sprite FPS validation while retaining invalid-draft, replay-drain and preview-lighting assertions.
- Separated bounded FORM compiler startup from its script execution budget, retaining accepted previews on failure.
- Clarified document/preview/override persistence, compacted FORM metadata and emitter ownership, and added view-only scene filtering plus optional particle replay windows.
- Added bounded property undo, cancellable gestures, fine/coarse numeric adjustment and repairable inline drafts; passive disk polling preserves in-progress edits.
- Unified trackpad camera gestures, viewport focus shortcuts and numeric-field scrolling across Level, FORM and particles.
- Added explicit Play destinations, with game preview closed at startup, plus compact workspace chrome, collapsible Assets and remembered panel sizing/maximize controls.

### Joy Engine VS Code extension
- Added an installable Joy Engine extension with JSON recognition and offline schemas for custom Joy formats, plus FormScript highlighting and editing support; uses the Joy Engine image as its extension icon.

### Portable editor assets
- Replaced per-project FORM, particle, and level adapter modules with declarative manifest settings; added shared schemas and VS Code associations for Joy asset formats.
- Moved every game's post-processing profile into a main `.joylevel`, allowing procedural games to use lightweight levels for shared scene settings without serializing generated entities.
- Renamed the workspace configuration to `joy.joyconfig`, with a dedicated `.joyconfig` schema and VS Code JSON association.

### Joy Editor content workflow
- Reworked asset creation into an inline type-and-name flow, added supported-format imports, moved find and file operations into the content browser, hid recovery management, and removed the six-tab workspace limit.

### Editor production availability
- Added per-editor production flags in `joy.config.json`: Joy Editor defaults to development-only, while FORM LAB and PARTICLE LAB remain enabled independently. Enabling Joy Editor includes both labs automatically.
- Applied the flags to production builds, site exports, editor previews, tool navigation and release captures, while retaining all editors in development.

## 09-30-2026
### Editor usability v1
- Added reusable `.joyobject` definitions, linked instances, explicit overrides, typed component authoring and undo/redo, with FORM/JOYFX asset attachment and source-editor navigation.
- Shared procedural asset presentation between the level editor and Vehicle Playground, with atomic content replacement and per-instance effect ownership.
- Added an owned multi-device input manager and migrated Vehicle Playground and Magic Colors; added shared live-object tuning, grouped edits, redo and presets.
- Unified workspace and hosted editor chrome around the intentional orange/blue palette, square controls and clearer editable surfaces.

### Rendering architecture
- Fixed FORM LAB preview startup on WebGPU and WebGL 2 by removing a duplicate PBR shader constant and completing the preview shaders' post-processing interface.
- Made the engine-owned `SceneRenderer` and postprocessor the default path for standard game and editor viewports, while retaining low-level project shaders only for behavior-specific effects.
- Migrated GOD GAME off its duplicate scene shaders and onto the shared scene renderer, enabled postprocessing for the FORM LAB viewport, and renamed the remaining SPACE SCUFFLES and BLAST GARDEN shader programs after their project-specific flash behavior.

### Editor integration
- Added project manifests and project-owned FORM, particle, or level capability adapters to every playable game, plus a shared preview connection for pause control and save-triggered content refreshes.
- Associated JSON-backed FORM LAB, JOYFX, level, and project files with JSON in the repository's VS Code settings.
- Fixed level-editor shader compilation and transparent rendering when post-processing is disabled; scene templates now emit fragment color without adding unused uniform fields.

### Joy Editor workspace
- Integrated project-owned manifests into workspace discovery, added desktop opening for compatible external folders, and added manifest-supported asset creation, rename, deletion, dependency navigation, and recovery-copy retention controls.
- Standardized BLAST GARDEN capture browser overrides with the repository capture environment contract and retired completed editor follow-up documents.

### Joy Engine 0.9.0
- Added data-driven entities and type registries, explicit world/resource ownership, possessable characters, player/AI controllers and safe possession transfer.
- Added versioned level documents, transactional authoring history, primitive scene presentation and shared level runtime instantiation.
- Added engine-owned cannon-es physics with configurable rigid bodies, material properties, queries, bounded fixed stepping and raycast vehicles with data-defined wheels, suspension, steering, drive and brakes.

### Joy Editor 0.2.0
- Made the JOY-ENGINE menu available in every game’s development session, with shared orange styling, hard corners and project-owned post-processing profiles.
- Added level documents to the shared workspace with exclusive new-file creation, direct saves, revision conflicts and malformed-source repair.
- Added a 3D level tool with an entity outliner, selection/placement, snapping, transform and component editing, duplication/deletion and undo/redo.
- Workspace saves stage authored content without Vite reloading a running game; restarting a preview still loads the latest saved modules.

## 09-29-2026
### Joy Engine 0.8.0
- Added immutable mesh assets, FORM asset conversion, persistent mesh instances and keyed scene collections with explicit ownership and cleanup.
- Added engine-owned frame preparation, cached instance transforms, frustum culling, shared scene shaders, lighting uniforms, transparency ordering, canvas resizing and camera projection/picking.
- Migrated LUNA and MAGIC COLORS to the shared scene layer while preserving gameplay and project-owned effect lifetimes. Retained the existing vertex-color triangle backend; hardware instancing and per-instance PBR rendering are not part of this pass.
- Removed the unused standalone particle-system APIs in favor of the asset-driven `ParticleEffect` runtime while retaining the motion helpers used by SPACE SCUFFLES.

## 09-28-2026
### FORM LAB scripting
- Retired `@attribute` syntax with migration diagnostics and require `wrangle <domain> in <geometry> as <binding>`; updated language examples, reference material, editor highlighting, and compiler regression coverage.

### Authoring lab examples
- Added Harvest grove, Patrol circuit, and Tactical terraces FORM studies with logical entity outputs, gameplay attributes, named user-data records, and plain-language comments explaining how games can use the data.
- Migrated every bundled FORM LAB study to current object-first and explicit-attribute scripting where applicable, and added inspectable user-data records for generated content.
- Migrated every bundled PARTICLE LAB effect and the new-emitter starter to aggregate vector and color scripting, with regression checks that compile all shipped examples.

## 09-25-2026
### FORM LAB scripting
- Require explicit attribute creation before wrangle reads or assignments, add domain-specific `addAttribute` declarations, and require `in` in named and implicit wrangles; migrated bundled examples and reference material.
- Added object-first box and sphere geometry, explicit translate/rotate/scale/color operations, and named wrangle element bindings while retaining compatibility with existing immediate-mode studies.
- Updated the terrain and reusable geometry recipes to avoid implicit color state and `@attribute` iteration syntax.

### Joy Editor project workspace
- Added Stop for running or paused project previews, preserving open asset tabs and unsaved edits.
- Restored the full project workspace entry point, including asset browsing, persistent FORM and particle editors, direct saves, conflict handling, and the live game preview.
- Browser and desktop editor commands now share the source-based development host; neither requires a production build or browser folder-selection support.
- Retained the manifest-adapter prototype as unfinished work without replacing the existing authoring workflow or advertising incomplete desktop packaging.

## 09-24-2026
### Joy Editor workspace
- Added a local project workspace with searchable content, persistent FORM/particle document tabs, direct file saves, external-change detection, conflict resolution, source repair, and recoverable replaced-file backups.
- Added `dev:editor` and an Electron workspace entry, retaining the standalone tools and their file formats.
- Connected God Game mesh/effect iteration to the running preview while staging world changes behind explicit regeneration; failed content retains or restores a working runtime result.
- Completed synchronous Node raw-loader registration so engine and editor tests can import authored FORM, effect, and shader assets.
- Removed the FORM LAB and PARTICLE LAB header marks and sourced each tool's version badge from its package metadata.
- Added a shared in-tool workspace switcher with direct tool navigation, current asset actions, keyboard shortcuts, and a route back to the experiment directory while retaining focused full-page tools.
- Renamed the shared editor root to `joy-editor/`, including tool URLs, npm workspace paths, and build/test tooling.
- Updated version list/check/bump commands to follow npm workspace paths, including editor tools, while ignoring leftover build folders.
- Serve FORM LAB and PARTICLE LAB at `/joy-editor/tools/<tool>/` in development and production, including landing-page links.
- Moved FORM LAB and PARTICLE LAB into `joy-editor/tools/`, with shared Joy Editor chrome and desktop-launch behavior at the editor root.
- Updated workspace discovery, site assembly, development routing, validation paths, and documentation for the editor tool layout.

## 2026-09-23
### FORM user data and lab previews
- Added scoped `with userData(record) { ... }` groups with nested restoration and local bindings; GOD GAME uses named multiline records beside each placement group.
- Added bounded nested user-data records to FORM points and editable geometry, retained through copies, runtime outputs, inspection, and JSON export; GOD GAME layouts now identify structures through data instead of color.
- Added shared preview fullscreen controls to FORM LAB and PARTICLE LAB, with Escape exit and a window-filling fallback.
- Hid FORM LAB workspace profile controls while retaining saved layouts and pane resizing.

## 2026-09-22
### FORM reusable workflows
- Added project-owned named parameter variants, including resource overrides, with save/apply/rename/delete controls and backward-compatible document persistence.
- Added runtime profile revolution, path tube sweeps, smooth-normal rebuilding, vessel/path workshops, and appendable starter recipes.

### FORM workspace
- Added persistent adjustable workspaces, keyboard pane resizing, inspector sections, parameter search, direct recipe access, and navigation to script errors.

### FORM procedural authoring
- Added reusable geometry, point/face/corner attributes, wrangle blocks, topology editing, vector math and noise, weighted scatter, retained instance collections, and named runtime outputs.
- Added linked result/attribute inspection, scalar/color visualization, a terrain workshop example, and searchable appendable recipes while preserving the FORM LAB workbench.
- Corrected WebGL PBR index offsets so assemblies with multiple materials render completely; documented the next layout and procedural workflow capabilities.

## 2026-09-18
### FORM runtime parameters
- FORM scripts can expose mesh/material inputs, construct reusable primitive resources, transform/deform geometry, and emit instances alongside existing scalar/vector/color parameters.
- Added the renderer-independent `joy-engine/form` runtime API and a parametric bridge example. FORM LAB retains its current editor layout with resource overrides and GLB geometry input.
- Logical instances currently bake into preview/export geometry; GLB inputs use the existing single-primitive geometry loader.

## 2026-09-16
### Platform, rendering, and validation
- Promoted the game and app experiment directory to the landing page's lead panel; Recent Updates now shows only the latest change with the viewer's local date and time.
- Reorganized Joy Engine rendering modules by camera, geometry, GPU, PBR, renderer, scene, and post-processing responsibilities; upgraded every luma.gl package to 9.4.1.
- Corrected the development renderer controls, including select inputs, conditional AgX settings, independent saturation and vibrance, multiplier-based brightness, Gaussian/scatter sliders, keyboard pause, Escape handling, and coordinated TUNE/JOY-ENGINE panels.
- Simplified routine validation by separating fast static checks from focused tests and removing specialized browser probes superseded by project capture workflows.
- Standardized particle asset filenames on `.joyfx`, retaining JSON content and shared raw-import support for builds and tests.
- Integrated loaders.gl GLB/glTF decoding, Shadertools assembly, and luma.gl image-effect pipelines with HDR multiscale bloom, anamorphic/lens shaping, animated grain, and portable WebGL 2 fallbacks. Added optional WebGPU SSAO, TAA, and motion blur with explicit frame inputs and owned history.
- Kept live POST FX saves from restarting Vite and pre-optimized lazy engine dependencies to avoid first-use game reloads.
- Restored SPACE SCUFFLES rendering on WebGPU and WebGL 2 by sizing scene uniform buffers from the complete post-processing data.
- Polished FORM LAB's material editor and preview controls, added image-backed PBR material channels and script UV/normal data, and retained flat face normals by default.

### FORM LAB materials and Joy Engine PBR
- Added reusable GPU metallic/roughness material rendering on WebGPU and WebGL 2, with six texture slots, owned resource cleanup, and camera redraws that retain uploaded geometry.
- Restored flat normals by default, added script-authored normals, and preserved hard edges and explicit normals in GLB export.
- Corrected and softened slider track borders, improved number fields, aligned parameter styling, added material deletion, thumbnails and color swatches, and removed the Alpha Cutoff control.

### FORM LAB v0.5.0
- Added GLB export with flat normals, vertex RGBA, material assignments, and authored point primitives.
- Added named material editing, script selection, document persistence, and constant `gltf_pbr` MaterialX import/export with explicit unsupported-feature errors.

### Development editor launchers
- Matched TUNE to the POST FX button styling and placed the launchers side by side, with TUNE on the left and POST FX on the right in both games.

### JOY ENGINE v0.4.6
- Added a Node-safe post-processing configuration entry point so Vite configs can use engine normalization without loading browser-only CSS and shader assets.
- Resolved the package export merge conflict while retaining both development editor entry points and the Node-safe post-processing entry point.
- Kept both games' tuning-config import chains Node-safe by exposing profile helpers through `joy-engine/post-processing`, with regression coverage for loading Vite configs without browser asset loaders.

## 2026-09-15
### FORM LAB v0.3.0
- Added script-owned project name, info, and title metadata, synchronized name-field edits, parameter syntax coloring, and RGBA preview swatches. Documented compiler bounds and lexical shadowing.
- Refined export controls, moved viewport options above generation results, relocated the backend label, fixed point-size slider fill, and restored visible double-click selections.

### JOY ENGINE v0.4.5
- Added shared syntax styling for exposed parameter symbols in lab editors.

### FORM LAB v0.2.0
- Added scalar, XYZ vector, and RGBA color parameters with named `meta(min=..., max=..., step=...)` declarations, typed controls, persisted overrides, and alpha-aware preview/PLY/JSON output. Existing scalar studies remain readable.
- Fit the desktop editor to one page; aligned document-name editing, parameter cards, action buttons, script help, copy, and generated/pending preview metrics with PARTICLE LAB.
- Aligned FORM LAB's document bar, controls, responsive workbench, editor surfaces, sliders, and status treatments with the current PARTICLE LAB tool-suite conventions while preserving its geometry-specific workflow.

### PARTICLE LAB refinements
- Refined the authoring layout, optional performance views, script help, destructive actions, parameter controls, and preview status.
- Added grouped scalar/vector/color properties with compact type selectors, editable component values, collapsible value controls with compact value/color previews, collapsible bounds, matching slider tracks, warmer action hovers, and neutral preview axes.
- Added typed vector and RGBA containers to Joy Engine particle scripts, whole-value assignments and math, component access, bounded color parameters, and documented persistent custom channels.

### PARTICLE LAB v0.1.0
- Add PARTICLE LAB for authoring layered scripted particle effects, exposed parameters, child emission, seeded preview, and portable effect assets.
- Add PARTICLE LAB desktop launch and Windows/macOS/Linux packaging through the Electron shell shared with FORM LAB.
- Clarify PARTICLE LAB editable surfaces, make JSON a read-only preview, default the seed to 7777, and add background/grid controls with per-emitter CPU and workload diagnostics.
- Share FORM LAB’s CodeMirror theme and editing controls with PARTICLE LAB, including syntax colors, line numbers, cursor status and undo history; improve workbench readability.
- Added texture controls (PNG-only at the moment) and a material example in PARTICLE LAB.
- Promote particle authoring to full XYZ motion and perspective rendering with camera-facing billboards, 3D velocity streaks, view-dependent transparency ordering, and an orbit camera shared with FORM LAB.
- Align PARTICLE LAB with FORM LAB’s branded shell, numbered compose/preview/tune panels, document bar, compact controls, and responsive workbench.

### JOY ENGINE v0.4.1
- Replace per-play HTML audio cloning with predecoded Web Audio samples, bounded overlapping voices, gesture activation, and explicit context cleanup to address Safari shooting hitches in SPACE SCUFFLES.

### JOY ENGINE v0.4.0
- Complete the game particle migration to `ParticleEffect`, preserving seeded motion and draw order, with explicit effect cleanup, exhaust-budget eviction, and world-position resizing.
- Add a root documentation hub with centralized project guides and a regenerable public engine API reference (`npm run docs:api`).
- Fix display-resolve GLSL startup by keeping the version directive first.
- Separated reusable engine shaders from project-owned `shaders/` trees, separated bloom filtering from the final display resolve, moved the renderer pipeline into `rendering/postprocessor`, and renamed DCC importers to distinguish them from project content assets.
- Clarified particle runtime ownership and added explicit browser-audio cleanup.
- Add scripted particle-attached point lights, reusable opaque scene lighting, and an Orbiting Lights example with a toggleable ground receiver.
- Add shared Joy Engine effect validation, bounded numeric scripting, deterministic simulation, and particle geometry for editor/game parity.
- Add solid circles, solid squares, textured sprites and animated flipbooks to Joy Engine particles (textures in the editor only support PNG at the moment).

## 2026-09-14
### Platform and Tooling
- Added repository-owned semantic-version management for the site, Joy Engine, and each game workspace, including manifest/lockfile consistency checks.
- Reorganized repository scripts by build, capture, development, maintenance, quality, and testing responsibility.
- Improved the landing-page mobile layout and retained versioned preview images for each playable project.
- Renamed project folders to consistent kebab-case names.

### Joy Engine 0.3.1
- Formalized Joy Engine as the dedicated `joy-engine` npm workspace and enforced the boundary between reusable engine code and project-specific behavior.
- Moved authored GPU programs into standalone WGSL and GLSL files.
- Added reusable development diagnostics with startup/backend logging, FPS and frame-time reporting, optional Chromium heap statistics, responsive placement, and explicit cleanup.
- Expanded the particle system with delayed particles, curved motion, position histories, child emitters, and deterministic random sources while retaining independent system ownership.
- Refined shared bloom, tone mapping, camera, buffer, mesh-loading, geometry, color, DOM, and simulation utilities.
- Completed a repository-wide organization, naming, documentation, and readability pass.

## 2026-09-12
### Joy Engine and effects
- Deepened the luma.gl integration and introduced shared HDR-aware post-processing with bloom, vignette, film grain, saturation, contrast, exposure, and tone-mapping controls.
- Added seeded effect playback and motion scrubbing to make visual simulation repeatable and easier to inspect.
- Significantly expanded explosions with layered cores, debris, smoke, trails, and secondary effects.
- Added shared particle-dynamics and rendering regression coverage.

## 2026-09-11
### Platform
- Added the generated sandbox landing page, chronological project cards, project preview captures, and site/engine/project version displays.
- Added Cloudflare Wrangler deployment support and adjusted release builds for hosted environments.
- Added automated browser captures, image galleries, diagnostic manifests, and last-good preview preservation.
- Added explicit high-DPI and responsive viewport handling.

### Joy Engine
- Reworked particles around project-agnostic emitters and update logic.
- Added GLB mesh loading, a proper top-down orthographic camera, depth buffering, opaque/translucent rendering rules, and blending fixes.
- Consolidated common simulation, input, audio, browser, geometry, and GPU ownership behind the engine API.

## 2026-09-09
### Joy Engine rendering and repository layout
- Expanded post-processing with punchy AgX, scalable higher-quality bloom, anamorphic flare shaping, vignette blur, configurable animated grain, and curated luma.gl effect exports.
- Moved authored JSON configuration and JOYFX assets out of project source trees, made capture folders chronologically sortable, preserved development console history, and documented explicit WebGPU/WebGL 2 development selection.
