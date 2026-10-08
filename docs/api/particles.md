# Particles API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## compileParticleEffect
Import from `joy-engine`. [Source](../../src/particles/asset.ts)

```ts
function compileParticleEffect(input: unknown): CompiledParticleEffect
```
Validate and compile a JSON-compatible effect into an immutable shared definition. Owns a normalized copy; callers can continue editing their source asset safely. Effects use caller-defined world units, seconds, radians and linear HDR RGB.

- **param** input


## ParticleEffect
Import from `joy-engine`. [Source](../../src/particles/particle-effect.ts)

Owns one effect's simulation, seed, parameters and particles; no host resources. Definition is borrowed and immutable. Particles are borrowed read-only views. World coordinates are XYZ; velocities/accelerations use seconds and drag is an exponential damping coefficient per second. Emission is cosmetic only. Optional random is borrowed: reset/seek continue that stream rather than rewinding it. Omit random for seed-reproducible editor reset and seek.

### constructor
```ts
constructor(definition: CompiledParticleEffect, options: ParticleEffectOptions = {})
```
- **param** definition
- **param** [options]

### particles
```ts
get particles()
```

### time
```ts
get time()
```
Elapsed fixed-step simulation time in seconds; excludes the fractional accumulator.


### isAlive
```ts
get isAlive()
```

### update
```ts
update(deltaSeconds: number)
```
Accumulate real time without making frame cadence change the simulation.

- **param** deltaSeconds Finite delta in [0, 0.25]. Owners clamp long pauses.

### step
```ts
step()
```
Advance one fixed tick. Survivors update before newly emitted children.


### reset
```ts
reset()
```
Reset time and particles at the current origin; emits time-zero bursts. Owned seeds rewind; borrowed random streams continue from their current state.


### seek
```ts
seek(seconds: number)
```
Replay to a fixed tick, bounded to 30 seconds; deterministic with an owned seed. Editors should schedule long seeks in chunks if responsiveness is required.

- **param** seconds

### removeOldestParticles
```ts
removeOldestParticles(count: number)
```
Evict oldest live particles for a session-wide cosmetic budget. Does not trigger death emitters or rewind the seed, clock, or total spawn count.

- **param** count Non-negative integer; excess count is harmless.

### scalePositions
```ts
scalePositions(scale: {
    x: number;
    y: number;
    z: number;
})
```
Rescale world positions about zero after a world-bounds resize. Velocity, size, time and force units stay unchanged. Future spawn origins follow the same transform; reset retains the resized origin. Validation is atomic.

- **param** scale Positive finite axis factors.

### stop
```ts
stop()
```
Stop root and child emission; live particles finish normally.


### destroy
```ts
destroy()
```
Idempotent terminal cleanup. The borrowed definition remains reusable.



## PARTICLE_STEP_SECONDS
Import from `joy-engine`. [Source](../../src/particles/particle-effect.ts)

```ts
const PARTICLE_STEP_SECONDS
```


## ParticleScriptError
Import from `joy-engine`. [Source](../../src/particles/script.ts)

Source location survives transport to an editor's diagnostic view.

### constructor
```ts
constructor(message: string, token: Token)
```
- **param** message
- **param** token


## appendParticleEffect
Import from `joy-engine`. [Source](../../src/particles/rendering.ts)

```ts
function appendParticleEffect(vertices: number[], effect: ParticleEffect, camera: ParticleCameraBasis)
```
Append camera-facing particles in full XYZ world space without advancing simulation. Coordinates and radius are world units. Colors are linear HDR straight alpha. The caller owns the batch and sorts it for the active camera before submission.

- **param** vertices Mutable interleaved XYZ/RGBA triangle list.
- **param** effect Borrowed runtime.
- **param** camera Orthonormal world-space camera basis. Sizes are world units.


## PARTICLE_XY_BASIS
Import from `joy-engine`. [Source](../../src/particles/rendering.ts)

```ts
const PARTICLE_XY_BASIS
```
Explicit fixed-plane basis for orthographic XY games.



## createParticleEffectBatches
Import from `joy-engine`. [Source](../../src/particles/rendering.ts)

```ts
function createParticleEffectBatches(effect: ParticleEffect, camera: ParticleCameraBasis, metrics?: ParticleRenderMetrics, additionalTriangles: readonly number[] | Float32Array = []): {
    vertices: number[];
    source?: string;
}[]
```
Create owned transparent material runs, sorted globally by triangle depth. Colored vertices have XYZ/RGBA stride 7; PNG runs add top-left-origin UV (stride 9). Consecutive triangles sharing a source are merged without changing blend order. Borrows effect/camera without mutation; no simulation or resource loading occurs.

- **param** effect
- **param** camera
- **param** [metrics] Optional owned record, reset and filled per call; emitter counts exclude additional geometry.
- **param** [additionalTriangles] Borrowed untextured XYZ/RGBA triangles, interleaved at camera depth without mutation.


## ParticleTextureSet
Import from `joy-engine`. [Source](../../src/particles/texture-set.ts)

Owns decoded particle textures for one renderer. The device is borrowed. Call prepare when the compiled definition changes; await it before drawing. Concurrent prepares discard obsolete work. Destroy before the GPU renderer.

### constructor
```ts
constructor(renderer: GpuTriangleRenderer)
```
- **param** renderer

### prepare
```ts
async prepare(definition: CompiledParticleEffect): Promise<boolean>
```
Atomically replace the texture set, retaining unchanged GPU allocations. Returns false if a newer prepare or destruction superseded this request.

- **param** definition

### activate
```ts
activate(definition: CompiledParticleEffect | null)
```
Commit the prepared definition; release images no longer drawn.

- **param** definition

### get
```ts
get(source: string)
```
Borrow a texture after a successful prepare.

- **param** source

### destroy
```ts
destroy()
```
Release owned GPU textures and invalidate any decode still in flight.



## PARTICLE_TEXTURE_SHADERS
Import from `joy-engine`. [Source](../../src/particles/texture-set.ts)

```ts
const PARTICLE_TEXTURE_SHADERS
```
Perspective texture shaders with mat4 viewProjection + vec4 tint uniforms. Compatible with GpuTriangleRenderer post processing. UV origin is top-left; sampled sRGB image colors become linear before multiplying HDR particle tint.



## DEFAULT_PARTICLE_TEXTURE
Import from `joy-engine`. [Source](../../src/particles/texture-source.ts)

```ts
const DEFAULT_PARTICLE_TEXTURE
```
Original procedural 16×16 white cross with transparent corners.



## collectParticleLights
Import from `joy-engine`. [Source](../../src/particles/lights.ts)

```ts
function collectParticleLights(effect: ParticleEffect, maxLights: number = 32): {
    lights: ParticlePointLight[];
    omitted: number;
}
```
Collect an owned bounded list without mutating simulation or consuming randomness. Lights follow XYZ positions; range is world units, RGB is linear HDR multiplied by intensity and clamped alpha. Rank by emitted luminance, then stable birth index. `omitted` counts valid emitting lights excluded by the requested cap (at most 32).

- **param** effect
- **param** [maxLights] Integer from 0 to 32; larger integers clamp to 32.


## createParticleRenderMetrics
Import from `joy-engine`. [Source](../../src/particles/rendering.ts)

```ts
function createParticleRenderMetrics(definition: CompiledParticleEffect)
```
Create caller-owned per-build CPU geometry metrics, with no GPU timing claims.

- **param** definition
