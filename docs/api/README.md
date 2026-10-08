# Joy Engine API reference
[Joy Engine README](../../README.md) · [Engine guide](../README.md)

The current public runtime surface is exported by [src/index.ts](../../src/index.ts). Import from `joy-engine`, not internal files. Development tooling, the project manifest and data-only constants have separate entry points; editor and Electron hosts live in Joy Editor, so games never import authoring code. Source links below point to the current checked-in implementation; generated signatures and contracts come from native TypeScript declarations and their documentation. Types can be imported with `import type` in TypeScript or referenced through `import('joy-engine').TypeName` in checked JavaScript.

## By subsystem
- [Types](types.md)
- [Core](core.md)
- [Browser](browser.md)
- [Runtime](runtime.md)
- [Environment](environment.md)
- [Rendering](rendering.md)
- [Gameplay](gameplay.md)
- [Physics](physics.md)
- [DCC interchange](dcc.md)
- [Particles](particles.md)
- [Project](project.md)

## Units and ownership
| API family | Units and lifecycle |
| --- | --- |
| Core and geometry | Caller-defined world coordinates; radians; linear RGBA unless stated otherwise. Geometry appenders mutate caller-owned arrays. |
| Motion | Velocity, acceleration, and drag are per fixed game tick. Functions mutate the supplied body. |
| ParticleEffect | Seconds, XYZ world coordinates, exponential drag coefficient per second. The instance owns simulation; the immutable definition is borrowed. Destroy on expiry/reset/teardown. |
| Rendering | Geometry is world-space; backing dimensions are pixels; colors are linear HDR before display resolve. Resource creators own cleanup. |
| Browser and runtime | Event timestamps are milliseconds; the fixed game loop schedules application updates. Keep returned stop/unbind callbacks. |

## Typical particle integration
```js
import { compileParticleEffect, ParticleEffect, PARTICLE_XY_BASIS, appendParticleEffect } from 'joy-engine';
import assetSource from './assets/joyfx/explosion.joyfx?raw';

const asset = JSON.parse(assetSource);

const definition = compileParticleEffect(asset);
const effect = new ParticleEffect(definition, { seed: 42, position: { x: 100, y: 200 } });
effect.step(); // Called by the owner's 60 Hz simulation update.
const transparent = [];
appendParticleEffect(transparent, effect, PARTICLE_XY_BASIS);
// Sort for the scene camera, then submit transparent through the scene renderer.
effect.destroy(); // At expiry or owner teardown.
```

For textures, profiling, script syntax and supported limits, use the [particle guide](../particles.md). For rendering initialization and display processing, use the [rendering guide](../overview.md) and [postprocessor guide](../postprocessing.md).

## Shader assets
The `joy-engine/shaders/*` package export exposes reusable authored WGSL/GLSL assets for raw-source loading. Scene-specific programs live in each project's `shaders/` directory and are imported locally. See [shader ownership](../shaders.md).

## Maintenance
Run `npm run docs:api` whenever exports, public types or documentation change. The generator resolves local re-exports, fails when an exported symbol is missing, and includes public class methods, native interfaces and type aliases. External re-exports are named with their package source. This is a source-derived reference, not a separately versioned API promise.
