# Rust Particles
The Rust `particles` module (rendering layer) ports the TypeScript `src/particles/` system: `.joyfx` documents, the particle script language, fixed-step simulation with seeded cosmetic randomness, particle point lights, and camera-facing geometry for the renderer's transparent queue, including textured and flipbook sprites. `rendering::AssetScene` ports `src/gameplay/asset-scene.ts` into the rendering layer, where level presentation belongs. The same `.joyfx` files load in both engines and simulate to the same particles for the same seed and inputs.

## Module Map
| TypeScript | Rust | Contents |
| --- | --- | --- |
| `particles/constants.ts` | `particles/constants.rs` | `ParticleRenderer`, `ParticleParameterType`, `ParticleValueKind` with the serialized spellings. |
| `particles/asset.ts` | `particles/asset.rs` | `compile_particle_effect`, `parse_particle_effect`, `CompiledParticleEffect`, `CompiledEmitter`, `ParticleEffectAsset` and its records, `ParticleParameterValue`, `ParticleAssetError`. |
| `particles/script.ts` | `particles/script.rs` | `ParticleScript` (`compile_particle_script`), `ParticleScriptError`, `ParticleScriptBudget`, and the field, input, function, and container tables. |
| `particles/particle-effect.ts` | `particles/particle_effect.rs` | `ParticleEffect`, `EffectParticle`, `ParticleEffectOptions`, `PARTICLE_STEP_SECONDS`, `particle_seed`, `ParticleEffectError`. |
| `particles/profile.ts` | `particles/profile.rs` | `ParticleProfile` and its create, reset, and live-refresh helpers. |
| `particles/lights.ts` | `particles/lights.rs` | `collect_particle_lights`, `ParticlePointLight` (converts to `rendering::PointLight`), `ParticleLightSelection`. |
| `particles/rendering.ts` | `particles/rendering.rs` | `append_particle_effect`, `create_particle_effect_batches`, `ParticleBatch`, `ParticleCameraBasis`, `PARTICLE_XY_BASIS`, `create_particle_render_metrics`. |
| `particles/texture-source.ts` | `particles/texture_source.rs` | `DEFAULT_PARTICLE_TEXTURE`, `read_particle_texture_source`, `encode_particle_texture_source` (PNG bytes to a source, for texture uploads), `ParticleTextureError`. |
| `particles/texture-set.ts` | `particles/texture_set.rs` | `ParticleTextureSet` (decoded images keyed by source) and its `transparent_draws` helper; `decode_particle_texture` decodes one source for previews. |
| `shaders/particles/textured.wgsl` | `rendering/shaders/textured.wgsl` | The textured transparent material, drawn by `rendering/materials/textured.rs`. |
| `particles/language-information.ts`, `particles/scripting.ts` | `particles/language_information.rs`, `particles/scripting.rs` | `particle_language_catalog` and the editor-facing `particles::scripting` entry point. |
| `gameplay/asset-scene.ts` | `rendering/asset_scene.rs` | `AssetScene`, `AssetSceneStage`, `EffectInstance`, `AssetSceneError`. |

`particles/math.rs` holds the JavaScript-compatible numeric helpers described under Determinism.

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

Effects without textured emitters can instead append colored triangles with `append_particle_effect` and pass them as `FrameInputs::transparent_triangles`, which the renderer sorts. Errors are typed and carry the TypeScript messages: script diagnostics keep their one-based line and UTF-16 column, and a failing step stops the effect and is returned again until `reset`.

## Renderer Hook
Textured particles needed a texture-capable transparent path; the hook is additive and kept out of the existing passes:
- `FrameInputs::transparent_draws: &[TransparentDraw]` (default empty). Runs draw after the frame's other transparency, in the supplied order without re-sorting, depth-tested against opaque geometry without depth writes, blending straight alpha source-over. `create_particle_effect_batches` already sorts particles (and any reference triangles passed to it) back to front across materials.
- `TransparentDraw::Colored(&[ColorVertex])` reuses the unlit blend pipeline through the renderer's transient buffer; `TransparentDraw::Textured { texture: &TextureAsset, vertices: &[TexturedVertex] }` uses the new textured material.
- `TexturedVertex` (position, color, UV; 36 bytes like the TypeScript stream) and `TextureAsset` (an immutable, cheaply cloned sRGB RGBA8 image with `from_rgba8` and `decode_png`) are new rendering records. Like `MeshAsset`, a texture uploads on first draw and its GPU copy is released once every handle is dropped, so particle code never holds device resources. `RenderError::InvalidTexture` reports malformed images, and `encode` fails before recording when a texture exceeds the device limit.
- `rendering/materials/textured.rs` (`TexturedPass`) owns the pipeline, sampler, vertex buffer, and texture uploads. Its layout is the frame uniforms, the shadow receiver (bound but unused), then the sprite texture, so the scene pass's first two groups stay bound across material switches. Sampling is sRGB with linear filtering and clamped edges; depth compare is `LessEqual` and blending matches the TypeScript textured pass. `renderer.rs` gained one field, one `prepare` call before the passes, the draw loop after the existing transient transparency, and a release call after the frame.
- `FormAssetLibrary::retain` lets `AssetScene` prune compiled meshes it no longer displays.

## AssetScene
`AssetScene` presents entities' `render` meshes (FORM and FORM LAB through `FormAssetLibrary`, drawn by `LevelScene`) and plays each `effect` component attached to its entity transform with the component's `seed` (default 7777), `loop`, `enabled`, and `parameters`. Asset text always comes from the caller's `read` function, so browsers can supply fetched text. Synchronization is staged: `prepare` resolves linked objects, reads every asset once, validates presentation records, compiles meshes and effects, decodes textures, and starts changed effects (one fixed step) without touching the running presentation; `commit` swaps the stage in unless a newer `prepare` or `cancel_pending` superseded it (returning `false`), and `sync` does both. A failed preparation leaves the displayed meshes, running effects, and caches unchanged. Effects whose asset text and settings are unchanged keep running across syncs. `step` advances effects (clamped to 0.1 s), restarts finished looping effects, and isolates failures in `error()`.

For the frame, `particles` returns world-space colored triangles for `transparent_triangles` (TypeScript parity), `particle_batches` with `transparent_draws` draws colored and textured particles sorted back to front across entities, and `particle_lights` and `lights` add world-space particle lights after the level's `light` components.

## Determinism
Simulation and geometry use `f64` like the TypeScript runtime, perform arithmetic in the same order, and draw from the same `SeededRandom` stream in the same order (scripts evaluate left to right, arguments before `rand()`, lazily for `&&` and `||`). Geometry is generated and depth-sorted in `f64` and converted to single precision only for the renderer.

Transcendental functions come from the `libm` crate (FreeBSD msun ports), so native and WebAssembly builds produce identical results. Against V8 (Node 22), `exp`, `tan`, `atan2`, `sqrt`, `floor`, `ceil`, `Math.hypot` (reimplemented with V8's scaled Kahan summation), JavaScript `Math.round`, `Math.min`/`Math.max` (NaN and signed zero), and `**` special cases match bit for bit. V8's `sin` and `cos` are a port of glibc's implementation, which an MIT engine cannot copy, and its `log` and `pow` also differ from FreeBSD's; each differs in the last bit for roughly one to four percent of inputs. In the recorded parity runs, 16,482 of 16,538 simulated values are bit-identical and the largest relative deviation is 7e-15; no count, emitter assignment, birth index, or message differs.

## Parity Tests
`test/fixtures/particles-parity.json` is recorded from the TypeScript runtime for the engine's `fire.joyfx`, eight game effects copied into `test/fixtures/joyfx/` (SPACE SCUFFLES explosion and color shift, BLAST GARDEN blast, FRAMEBREAK blade, boost, and wreck, LUNA lightning, and VECTORS TO VICTORY torch), and the engine-authored `lantern.joyfx` (lights, textured and flipbook renderers, trails, deaths, and vector and color parameters). `test/rust/particles_parity.rs` compares:
- Normalized assets (`ParticleEffectAsset::to_json`) structurally.
- Particle counts, emitted and dropped totals, liveness, and sampled particles (every field, origins, generations, and birth indices) at five to seven fixed steps per effect, within 1e-9 relative.
- Batch counts, materials, vertex counts, sums, and leading vertices for perspective and XY cameras, and `append_particle_effect` output, within 2e-6 relative after single-precision conversion.
- Particle lights (ranking, values, omitted counts) and 30 Hz frame-cadence results.
- 54 scripts: evaluation results (including locals) and diagnostics with lines and columns, including budgets, nesting, token and length limits, and comments.
- 67 invalid documents and 9 runtime failures, by exact message.

`test/rust/particles_asset_scene.rs` also replays a recorded TypeScript `AssetScene` run (three attached effects with rotated, scaled transforms, one failing mid-run) and compares effect ticks, counts, errors, and particle geometry per frame. Behavior tests port the TypeScript particle suites (`particles_effect.rs`, `particles_rendering.rs`), and `particles_headless.rs` renders textured and colored particles over an occluding block on a headless GPU (skipped without an adapter).

## Differences From TypeScript
- **End-of-script errors.** An expression that reaches the end of a script (`x = 1 +`) throws a host `TypeError` without a location in TypeScript; Rust reports `Unexpected end of script` at the end position.
- **Typed inputs.** Seeds are `i64` and counts `usize`, so non-integer seeds and negative counts cannot be passed; `particle_seed` validates JavaScript-number seeds from authored JSON with the TypeScript message. A borrowed game random stream is passed as a `Box<dyn RandomSource>`; wrap a shared stream to keep using it elsewhere.
- **Texture sets.** PNG decoding is synchronous (the `png` crate, matching `createImageBitmap` without color conversion or premultiplication), so a prepare cannot be superseded and `destroy` is `clear`. Sets hold CPU images; the renderer owns GPU copies. `PARTICLE_TEXTURE_SHADERS` has no counterpart because the renderer owns the textured material; GLSL variants are unnecessary with wgpu, and the always-white `tint` uniform was dropped.
- **Geometry inputs.** Additional reference triangles for `create_particle_effect_batches` are `ColorVertex` values (single precision) rather than JavaScript number arrays.
- **AssetScene.** Synchronization is synchronous with an explicit `prepare`/`commit` split; mesh instances live in a caller-owned `RenderScene` passed to each call, and `destroy` takes that scene. Textured effects are supported instead of rejected, and particle lights are available. `render` and `light` records are validated during `prepare` (Rust's `LevelScene` reports invalid records). `particles` omits textured emitters, which `particle_batches` draws. Entity transforms are single precision in Rust, so authored transforms round to `f32` before attaching particles. `Number(seed)` coercion treats arrays and objects as NaN.
- **Diagnostics order.** JavaScript orders integer-like object keys first, so for documents with several invalid parameter names TypeScript may report a different one first; such keys are never valid identifiers. A non-BMP character outside comments is quoted whole rather than as a lone surrogate.
