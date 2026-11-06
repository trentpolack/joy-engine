# Rust Particles
`joy_engine::particles` combines two crates: `joy-fx` owns `.joyfx` documents, the numeric script language, and fixed-step simulation with seeded cosmetic randomness; the renderer's `particles` module (`joy-renderer`) turns effects into point lights and GPU-instanced particles, including textured and flipbook sprites. `joy-scene`'s `AssetScene` integrates effects with authored levels. Native and browser builds use the same definitions and simulation.

## Module Map
Paths are relative to `crates/`.

| Rust | Contents |
| --- | --- |
| `joy-fx/src/constants.rs` | `ParticleRenderer`, `ParticleBlend`, `ParticleParameterType`, `ParticleValueKind` with the serialized spellings. |
| `joy-fx/src/asset.rs` | `compile_particle_effect`, `parse_particle_effect`, `CompiledParticleEffect`, `CompiledEmitter` (with its compact `ParticleTextureId`), `ParticleEffectAsset` and its records, `ParticleParameterValue`, `ParticleAssetError`. |
| `joy-fx/src/script.rs` | `ParticleScript` (`compile_particle_script`), `ParticleScriptError`, `ParticleScriptBudget`, and the field, input, function, and container tables. |
| `joy-fx/src/particle_effect.rs` | `ParticleEffect`, `EffectParticle`, `ParticleEffectOptions`, `PARTICLE_STEP_SECONDS`, `particle_seed`, `ParticleEffectError`. |
| `joy-fx/src/particle_pool.rs` | `ParticleEffectPool`, `PooledEffect`, `EffectPoolOptions`, `MAXIMUM_POOL_UPDATE_SECONDS`. |
| `joy-fx/src/profile.rs` | `ParticleProfile` and its create, reset, and live-refresh helpers. |
| `joy-renderer/src/particles/lights.rs` | `collect_particle_lights`, `ParticlePointLight` (a shared `PointLight` plus the emitter and particle it came from), `ParticleLightSelection`. |
| `joy-renderer/src/particles/frame.rs` | `ParticleFrame` (the one particle submission), `ParticleBatch`, `ParticleRenderError`. |
| `joy-renderer/src/particles/instance.rs` | `ParticleInstance` (the 60-byte GPU record) and the shape, roll, trail, and flipbook-cell rules that fill it. |
| `joy-fx/src/texture_source.rs` | `DEFAULT_PARTICLE_TEXTURE`, `read_particle_texture_source`, `encode_particle_texture_source` (PNG bytes to a source, for texture uploads), `ParticleTextureError`. |
| `joy-renderer/src/particles/texture_set.rs` | `ParticleTextureSet` (decoded images keyed by `ParticleTextureId`); `decode_particle_texture` decodes one source for previews. |
| `joy-renderer/src/pipelines/particles.rs` | The GPU pass both renderers share: culling, per-particle ordering, the instance buffer, texture uploads, and the source-over and additive pipelines. |
| `joy-renderer/src/shaders/particle-shapes.wgsl` | Quad expansion and the analytic shapes; `particle-scene.wgsl` and `particle-triangles.wgsl` add each renderer's bindings and entry points. |
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
textures.prepare(&definition)?;                                 // ParticleTextureSet: decode PNGs before drawing
textures.activate(Some(&definition));
particles.clear();                                              // a ParticleFrame the owner keeps between frames
particles.add_effect(&effect, Some(&textures))?;               // `None` for effects without textured emitters
target.draw(&scene, &camera, &FrameInputs { lights: &lights, particles: Some(&particles), ..Default::default() })?;
```

Errors are typed and preserve stable authored diagnostics: script diagnostics keep their one-based line and UTF-16 column, and a failing step stops the effect and is returned again until `reset`. Adding an effect fails, leaving the frame unchanged, only when a textured emitter's image is not in the texture set.

## Effect Pools
Games that play effects as one-shot cues (impacts, casts, and deaths) keep them in a `ParticleEffectPool<T>`, where `T` is the game's data for each effect (what played it, a color, or a light). The pool evicts the oldest effect when its `capacity` is full, gives each `spawn` the next value of its own cosmetic seed counter (`first_seed`), and drops effects that finish. `update(seconds)` clamps the frame to `MAXIMUM_POOL_UPDATE_SECONDS`; fixed-tick owners call `step()` instead. An effect whose script fails is logged and dropped, so one broken cue never stops a frame. `clear` keeps the seed counter running and `restart` rewinds it, so a restarted level can replay the same effects. Pass complete `ParticleEffectOptions` to `spawn_with` for parameter overrides or a game-owned seed, and draw `effects()`.

```rust
let mut effects = ParticleEffectPool::new(EffectPoolOptions { capacity: 24, first_seed: 41 });
effects.spawn(&definitions.impact, [event.x, 0.5, event.z], ImpactCue { color })?;
effects.update(frame_seconds);
for row in effects.iter() {
    particles.add_effect(&row.effect, None)?; // row.age_seconds for flashes, row.data for the game's cue
}
```

## Instanced Drawing
Each visible particle becomes one `ParticleInstance` (60 bytes in five vertex attributes, within WebGL 2's sixteen): world position and size, linear RGB and alpha, a streak's world trail offset and the camera-facing roll, a sprite's atlas cell as 16-bit texture coordinates, and the shape id with a premultiplied flag. Each instance draws a shared six-vertex quad generated from the vertex index (`VertexStepMode::Instance`). The vertex shader faces the quad to the camera (the scene renderer reads the camera's axes from the frame's view matrix; triangle frames supply theirs) and, for streaks, lays it along the trail projected onto the view plane. The fragment shader computes the shape analytically and returns premultiplied color, so straight-alpha and premultiplied emitters share one source-over pipeline and additive emitters use a one-one pipeline.

`ParticleFrame` groups each effect's emitters by blend family and texture into batches with world bounds, writing instances in generation order with a counting sort, and keeps every allocation between frames; textures are referenced by the 64-bit `ParticleTextureId` each compiled emitter carries, so no frame hashes or compares base64 source text. The renderer culls whole batches whose bounds fall outside the view (`RendererDiagnostics::particle_batches_culled`), sorts each source-over batch's particles back to front with one key per particle (equal depths keep generation order; additive batches are not sorted), and gives each batch one place in the transparency queue at the view depth of its bounds center. Fog dims additive light and fogs source-over particles like any surface.

`TriangleRenderer` draws the same batches with the same shaders through `TriangleFrame::particles` (`TriangleParticles`): the game supplies its camera matrix, billboard axes, how many of its transparent triangles draw before the particles, and its color convention (`display_colors` decodes sRGB-authored values like the games' own shaders, and `color_scale` applies a frame flash). Images upload with mip chains on first draw and their GPU copies are released once every `TextureAsset` handle is dropped, so particle code never holds device resources; a frame fails before recording when an image exceeds the device limit.

## AssetScene
`AssetScene` presents entities' `render` meshes (FORM and FORM EDITOR through `FormAssetLibrary`, drawn by `LevelScene`) and plays each `effect` component attached to its entity transform with the component's `seed` (default 7777), `loop`, `enabled`, and `parameters`. Asset text always comes from the caller's `read` function, so browsers can supply fetched text. Synchronization is staged: `prepare` resolves linked objects, reads every asset once, validates presentation records, compiles meshes and effects, decodes textures, and starts changed effects (one fixed step) without touching the running presentation; `commit` swaps the stage in unless a newer `prepare` or `cancel_pending` superseded it (returning `false`), and `sync` does both. A failed preparation leaves the displayed meshes, running effects, and caches unchanged (`FormAssetLibrary::retain` prunes compiled meshes it no longer displays). Effects whose asset text and settings are unchanged keep running across syncs. `step` advances effects (clamped to 0.1 s), restarts finished looping effects, and isolates failures in `error()`.

For the frame, `add_particles` adds every entity's effect (colored and textured) to a `ParticleFrame` at its entity transform (sizes scale with the transform's mean axis scale; billboards stay camera-facing), and `particle_lights` and `lights` add world-space particle lights after the level's `light` components.

## Determinism
Simulation uses `f64`, performs arithmetic in the same order, and draws from the instance-owned `SeededRandom` stream in the same order (scripts evaluate left to right, arguments before `rand()`, lazily for `&&` and `||`). Drawing is presentation: instances are single precision, and their order and shapes never feed back into the simulation.

Transcendental functions go through `joy_core::math::deterministic` (portable `libm`), so native and WebAssembly builds agree bit for bit; `min`, `max`, `round` (halves away from zero), and `clamp` are Rust's own. Script conditions treat zero and NaN as false. Script lengths and diagnostic columns count characters. Recorded reference fixtures compare simulation within explicit numerical tolerances, preserving particle counts, ordering, IDs, messages, and lights without maintaining another runtime; how particles are drawn is reviewed through captures instead.

## Tests and Boundaries
`crates/joy-renderer/tests/integration/particles_parity.rs` validates normalized assets, fixed-step results, light selection, script diagnostics, and rejected documents against recorded fixtures. `joy-scene`'s `particles_asset_scene.rs` verifies staged level effects; behavior suites cover lifetime, batching, textures, and flipbook frames, and headless rendering (`particles_headless.rs`) checks that every shape covers its particle in every blend mode, that particles sort back to front, cull outside the view, and hide behind opaque geometry, and that triangle frames draw them at their layer in the game's color convention (`JOY_RENDER_CAPTURE_DIR` keeps the captures).

Seeds use `i64` and counts use `usize`; authored JSON is validated before conversion. Texture decoding is synchronous and texture sets own CPU images while the renderer owns GPU copies. Effects support embedded PNG textures; particle instances are converted to single precision when they are built. `AssetScene::prepare` validates and stages all changes before `commit`; failures keep accepted presentation intact.
