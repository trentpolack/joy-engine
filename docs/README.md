# Joy Engine
[Documentation hub](https://github.com/trentpolack/JoyGames/blob/develop/docs/README.md) · [API reference](api/README.md)

Joy Engine is the repository's browser-native JavaScript package. Games import runtime services from `joy-engine`. Authoring UI (the CodeMirror script editor, preview fullscreen) and the Electron lab host live in Joy Editor (`@joy-games/joy-editor`, `@joy-games/joy-editor/desktop`), so the engine carries no editor dependencies. Each workspace uses the engine source from the same repository revision.

## Read next
- [Gameplay, levels and physics](gameplay.md): possession, data-driven entities, level authoring and tunable vehicles.
- [Public API reference](api/README.md): signatures, JSDoc contracts, and source links grouped by subsystem.
- [Rendering architecture](overview.md): triangle batches, depth, postprocessing, cameras, and browser resources.
- [Particle effects](particles.md): editable assets and the shared simulation runtime.
- [Audio](audio.md): decoded sound effects, gesture activation, overlap limits, and cleanup.
- [Postprocessing](postprocessing.md), [shaders](shaders.md), and [DCC interchange](dcc.md).
- [Environment simulation and rendering](environment.md): solar time, wind, atmosphere, clouds, light shafts, fog, and shared GPU data.

## Ownership and lifetime
Create browser/GPU resources before starting the frame loop. A session owns its input, audio, effects, renderer, and stop callback. Dispose the loop and listeners first, then effects and textures, then the renderer/device. Renderers borrow frame state; they must never advance simulation or consume gameplay randomness. Keep independent sessions free of mutable module-level state.

Games own gameplay, palettes, assets, camera policy, and effect composition. Joy Engine owns the standard scene shaders and portable rendering mechanisms and may not import game code. A project owns a shader only when a named material or effect requires behavior that the standard scene contract cannot express. WebGPU is preferred; WebGL 2 is the implemented fallback. WebGL 1 is not currently implemented.

## Particle integration
SPACE SCUFFLES uses `ParticleEffect` for explosions, exhaust, impacts, and color-change bursts. BLAST GARDEN uses it for all debris; its pressure waves remain gameplay entities because they trigger cultivation. PARTICLE LAB previews the same runtime. FORM LAB authors geometry and has no particle simulation to migrate.

Games call `step()` once per 60 Hz tick and destroy expired effects, reset effects on a new session, and release them on application teardown. Recipes live in each game's `assets/joyfx/*.joyfx`; project rendering preserves the existing silhouettes and mesh debris. A render adapter translates seconds-based particle state to presentation values without integrating motion itself.

## Checks and documentation maintenance
Run `npm run check`, `npm test`, and `npm run validate` from the repository root after engine changes. Regenerate the public API reference with `npm run docs:api`; review JSDoc and regenerate whenever an exported contract changes. The generator resolves every local re-export and omits methods marked `@private`.

## Lab editor symbols
The shared editor theme supports `variableName.special` stream-language tokens for exposed parameters, using a distinct cyan color. Each lab owns declaration discovery and language semantics; the shared editor owns the visual treatment and selection layer.

- [Mesh assets and rendering scenes](scenes.md): retained instances, frame preparation, ownership and migration examples.
