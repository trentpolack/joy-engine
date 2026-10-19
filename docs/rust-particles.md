# Rust Particles
`joy_engine::particles` combines two crates: `joy-fx` owns `.joyfx` documents, the numeric script language, and fixed-step simulation with seeded cosmetic randomness; the renderer's `particles` module (`joy-renderer`) turns effects into point lights and camera-facing geometry, including textured and flipbook sprites. `joy-scene`'s `AssetScene` integrates effects with authored levels. Native and browser builds use the same definitions and simulation.

## Module Map
Paths are relative to `crates/`.

| Rust | Contents |
| --- | --- |
| `joy-fx/src/constants.rs` | `ParticleRenderer`, `ParticleParameterType`, `ParticleValueKind` with the serialized spellings. |
| `joy-fx/src/asset.rs` | `compile_particle_effect`, `parse_particle_effect`, `CompiledParticleEffect`, `CompiledEmitter`, `ParticleEffectAsset` and its records, `ParticleParameterValue`, `ParticleAssetError`. |
| `joy-fx/src/script.rs` | `ParticleScript` (`compile_particle_script`), `ParticleScriptError`, `ParticleScriptBudget`, and the field, input, function, and container tables. |
| `joy-fx/src/particle_effect.rs` | `ParticleEffect`, `EffectParticle`, `ParticleEffectOptions`, `PARTICLE_STEP_SECONDS`, `particle_seed`, `ParticleEffectError`. |
| `joy-fx/src/profile.rs` | `ParticleProfile` and its create, reset, and live-refresh helpers. |
| `joy-renderer/src/particles/lights.rs` | `collect_particle_lights`, `ParticlePointLight` (converts to `rendering::PointLight`), `ParticleLightSelection`. |
| `joy-renderer/src/particles/batches.rs` | `append_particle_effect`, `create_particle_effect_batches`, `ParticleBatch`, `ParticleCameraBasis`, `PARTICLE_XY_BASIS`, `create_particle_render_metrics`. |
| `joy-fx/src/texture_source.rs` | `DEFAULT_PARTICLE_TEXTURE`, `read_particle_texture_source`, `encode_particle_texture_source` (PNG bytes to a source, for texture uploads), `ParticleTextureError`. |
| `joy-renderer/src/particles/texture_set.rs` | `ParticleTextureSet` (decoded images keyed by source) and its `transparent_draws` helper; `decode_particle_texture` decodes one source for previews. |
| `joy-renderer/src/shaders/textured.wgsl` | The textured transparent material, drawn by `joy-renderer/src/materials/textured.rs`. |
| `joy-fx/src/language_information.rs`, `joy-fx/src/scripting.rs` | `particle_language_catalog` and the editor-facing `particles::scripting` entry point. |
| `joy-scene/src/asset_scene.rs` | `AssetScene`, `AssetSceneStage`, `EffectInstance`, `AssetSceneError`. |

## Using Effects
Compile a document once and share the definition; each `ParticleEffect` owns its particles, seed, parameters, and clock:

```rust
use joy_engine::prelude::*;

let definition = parse_particle_effect(&effect_text)?;
let mut effect = ParticleEffect::new(&definition, ParticleEffectOptions { seed: 7777, position: [0.0, 1.0, 0.0], ..Default::default() })?;
effect.update(frame_seconds.min(0.25))?;                       // whole 1/60 s steps; cadence never changes the result
let lights = collect_particle_lights(&effect, 8).point_lights(); // FrameInputs::lights
let batches = create_particle_effect_batches(&effect, &ParticleCameraBasis::from(camera.basis()), None, &[])?;
textures.prepare(&definition)?;                                 // ParticleTextureSet: decode PNGs before drawing
textures.activate(Some(&definition));
let draws = textures.transparent_draws(&batches)?;
target.draw(&scene, &camera, &FrameInputs { lights: &lights, transparent_draws: &draws, ..Default::default() })?;
```

Effects without textured emitters can instead append colored triangles with `append_particle_effect` and pass them as `FrameInputs::transparent_triangles`, which the renderer sorts. Errors are typed and preserve stable authored diagnostics: script diagnostics keep their one-based line and UTF-16 column, and a failing step stops the effect and is returned again until `reset`.

## Renderer Hook
Textured particles needed a texture-capable transparent path; the hook is additive and kept out of the existing passes:
- `FrameInputs::transparent_draws: &[TransparentDraw]` (default empty). Runs draw after the frame's other transparency, in the supplied order without re-sorting, depth-tested against opaque geometry without depth writes, blending straight alpha source-over. `create_particle_effect_batches` already sorts particles (and any reference triangles passed to it) back to front across materials.
- `TransparentDraw::Colored(&[ColorVertex])` reuses the unlit blend pipeline through the renderer's transient buffer; `TransparentDraw::Textured { texture: &TextureAsset, vertices: &[TexturedVertex] }` uses the new textured material.
- `TexturedVertex` (position, color, UV; 36 bytes per vertex) and `TextureAsset` (an immutable, cheaply cloned sRGB RGBA8 image with `from_rgba8` and `decode_png`) are new rendering records. Like `MeshAsset`, a texture uploads on first draw and its GPU copy is released once every handle is dropped, so particle code never holds device resources. `RenderError::InvalidTexture` reports malformed images, and `encode` fails before recording when a texture exceeds the device limit.
- `joy-renderer/src/materials/textured.rs` (`TexturedPass`) owns the pipeline, sampler, vertex buffer, and texture uploads. Its layout is the frame uniforms, the shadow receiver (bound but unused), then the sprite texture, so the scene pass's first two groups stay bound across material switches. Sampling is sRGB with linear filtering and clamped edges; depth compare is `LessEqual` with straight-alpha blending. `renderer.rs` gained one field, one `prepare` call before the passes, the draw loop after the existing transient transparency, and a release call after the frame.
- `FormAssetLibrary::retain` lets `AssetScene` prune compiled meshes it no longer displays.

## AssetScene
`AssetScene` presents entities' `render` meshes (FORM and FORM LAB through `FormAssetLibrary`, drawn by `LevelScene`) and plays each `effect` component attached to its entity transform with the component's `seed` (default 7777), `loop`, `enabled`, and `parameters`. Asset text always comes from the caller's `read` function, so browsers can supply fetched text. Synchronization is staged: `prepare` resolves linked objects, reads every asset once, validates presentation records, compiles meshes and effects, decodes textures, and starts changed effects (one fixed step) without touching the running presentation; `commit` swaps the stage in unless a newer `prepare` or `cancel_pending` superseded it (returning `false`), and `sync` does both. A failed preparation leaves the displayed meshes, running effects, and caches unchanged. Effects whose asset text and settings are unchanged keep running across syncs. `step` advances effects (clamped to 0.1 s), restarts finished looping effects, and isolates failures in `error()`.

For the frame, `particles` returns world-space colored triangles for `transparent_triangles`, `particle_batches` with `transparent_draws` draws colored and textured particles sorted back to front across entities, and `particle_lights` and `lights` add world-space particle lights after the level's `light` components.

## Determinism
Simulation and geometry use `f64`, perform arithmetic in the same order, and draw from the instance-owned `SeededRandom` stream in the same order (scripts evaluate left to right, arguments before `rand()`, lazily for `&&` and `||`). Geometry is generated and depth-sorted in `f64` and converted to single precision only for the renderer.

Transcendental functions go through `joy_core::math::deterministic` (portable `libm`), so native and WebAssembly builds agree bit for bit; `min`, `max`, `round` (halves away from zero), and `clamp` are Rust's own. Script conditions treat zero and NaN as false. Script lengths and diagnostic columns count characters. Recorded reference fixtures compare simulation within explicit numerical tolerances, preserving particle counts, ordering, IDs, messages, geometry, and lights without maintaining another runtime.

## Tests and Boundaries
`crates/joy-renderer/tests/integration/particles_parity.rs` validates normalized assets, fixed-step results, camera geometry, light selection, script diagnostics, and rejected documents against recorded fixtures. `joy-scene`'s `particles_asset_scene.rs` verifies staged level effects; behavior and headless rendering suites cover lifetime, ordering, textures, and occlusion.

Seeds use `i64` and counts use `usize`; authored JSON is validated before conversion. Texture decoding is synchronous and texture sets own CPU images while the renderer owns GPU copies. Effects support embedded PNG textures; particle geometry is converted to single precision only at the renderer boundary. `AssetScene::prepare` validates and stages all changes before `commit`; failures keep accepted presentation intact.
