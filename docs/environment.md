# Environment simulation and rendering
[Engine guide](README.md) · [Rendering architecture](overview.md) · [API reference](api/README.md)

`EnvironmentSystem` is the engine-owned, renderer-independent source of truth for a prototype's outdoor environment. It advances normalized local solar time, calculates sun and moon directions from latitude and season, selects the primary atmosphere light, derives linear skylight, and produces a deterministic gusting wind field. It also carries validated atmospheric scattering, height-fog, cloud, light-shaft, and ambient-occlusion policy in one immutable per-frame snapshot.

The system does not own GPU resources, browser listeners, or a frame loop. A game owns one instance, calls `step(deltaSeconds)` with real elapsed seconds, and borrows the resulting snapshot for gameplay and rendering. A day length of zero pauses solar time while wind continues to evolve. Wind is analytic rather than random, so visual sampling cannot consume gameplay randomness and equal clocks produce equal frames.

```js
import {
  EnvironmentSystem,
  createEnvironmentLighting,
  createEnvironmentUniforms
} from 'joy-engine';

const environment = new EnvironmentSystem({
  latitudeDegrees: 42,
  dayLengthSeconds: 1200,
  clouds: { coverage: 0.55 },
  atmosphere: { fogDensity: 0.003 }
});

const environmentFrame = environment.step(deltaSeconds);
const pbrLighting = createEnvironmentLighting(environmentFrame);
pbrTriangles.prepare({ ...cameraFrame, ...pbrLighting });

const environmentUniforms = createEnvironmentUniforms(environmentFrame);
environmentUniformBuffer.write(environmentUniforms);
```

## Rendering contract
`createEnvironmentLighting` adapts the active sun or moon and skylight directly to the standard PBR frame. `createEnvironmentUniforms` packs a 256-byte, 16-byte-aligned block for higher-fidelity environment renderers. It includes both celestial lights, skylight, wind velocity and elapsed time, scattering and fog controls, cloud layer geometry and shadows, light-shaft controls, ambient-occlusion controls, and time of day.

All directions use Y-up world space. Cloud altitude, thickness, fog distance, and ambient-occlusion radius use the consumer's world units. Colors and light values are linear and should enter the scene before display exposure and tone mapping. GPU owners allocate and destroy their buffers and textures; snapshots and packed arrays own no GPU resources.

Atmosphere, volumetric-cloud, fog, and light-shaft passes should share this block rather than maintaining independent clocks or sun directions. SSAO remains available through `SceneEffects`; the environment's ambient-occlusion fields let a game keep the broader environment preset together while translating those fields into `SceneEffects` configuration at creation time.

## V1 boundaries
This first contract establishes synchronized simulation data and portable GPU layout without prescribing a single sky mesh, noise texture, or ray-march budget. Renderers may scale sample counts by backend while preserving the same world state. WebGPU remains the preferred path and WebGL 2 renderers can consume the same packed values with a uniform buffer. Unsupported expensive passes should fall back to the PBR atmosphere light, skylight, and height-fog values rather than changing simulation.
