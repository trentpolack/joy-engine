# Scripted Particle Effects

`compileParticleEffect(json)` validates a version 1 document and returns a reusable immutable definition. `new ParticleEffect(definition, options)` creates an independently owned simulation; definition sharing never shares particle state. Randomness is instance-seeded unless a caller explicitly supplies a borrowed stream. `appendParticleEffect(vertices, effect, camera.basis())` produces the same geometry for editor previews and game rendering.

```js
import { compileParticleEffect, ParticleEffect, appendParticleEffect, OrbitCamera, sortCameraTransparentTriangles } from 'joy-engine';

const definition = compileParticleEffect(await response.json());
const effect = new ParticleEffect(definition, {
  seed: 42,
  position: { x: 120, y: 240, z: 0 },
  parameters: { power: 1.5 },
});

// In a 60 Hz simulation; otherwise use update(deltaSeconds) with deltas <= 0.25.
effect.step();
const camera = new OrbitCamera();
camera.distance = 650;
const transparentVertices = [];
appendParticleEffect(transparentVertices, effect, camera.basis());
const sorted = sortCameraTransparentTriangles(transparentVertices, camera.basis().backward);
// Submit sorted as the transparent batch with camera.matrix(aspect, backend === 'webgpu').

// Stop all future emission, allowing existing particles to drain.
effect.stop();
// At owner disposal:
effect.destroy();
```

Particles live in full XYZ world space. Position, velocity, acceleration, spawn origins, inherited parent velocities, and child emission operate on all three axes. The editor uses a Y-up perspective orbit camera. World units belong to the application; velocity is units/second, acceleration units/second², angles radians, and drag an exponential damping coefficient per second. RGB is linear HDR; alpha is clamped for rendering.

`appendParticleEffect` requires an orthonormal camera basis (`right`, `up`, `backward`). Soft particles, rings, and billows face that camera; `rotation` and `spin` describe roll within the billboard plane. Streak tails follow the full world velocity, including Z. Perspective projection controls apparent size; sprites are sized in world units, not fixed screen pixels. The borrowed runtime is never mutated by drawing. Sort transparent triangles with `sortCameraTransparentTriangles` for the active camera; centroid sorting remains approximate for intersecting translucent surfaces.

`PARTICLE_XY_BASIS` explicitly selects fixed XY-facing geometry for existing orthographic games such as SPACE SCUFFLES. The engine does not assume that Z is a presentation layer. FORM LAB and PARTICLE LAB both use the shared `OrbitCamera`; geometry producers select the appropriate camera basis.

## Asset Structure
```json
{
  "version": 1,
  "name": "Small burst",
  "maxParticles": 256,
  "parameters": { "power": { "value": 1, "min": 0.1, "max": 4 }, "direction": { "type": "vector3", "value": [0, 1, 0], "min": [-1, -1, -1], "max": [1, 1, 1] } },
  "emitters": [{
    "id": "sparks", "enabled": true, "root": true,
    "duration": 0, "rate": 0, "bursts": [{ "time": 0, "count": 24 }],
    "lifetime": 0.75, "renderer": "streak",
    "spawn": "let angle = rand(0, tau); vx = cos(angle) * 180 * power; vy = sin(angle) * 180 * power; size = 2; r = 4; g = 1; b = 0.1;",
    "update": "alpha = 1 - t; drag = 1.2;"
  }]
}
```

Every emitter has its own spawn/update programs. Root emitters use a finite continuous rate and scheduled bursts. A duration-zero root can burst at time zero. `root: false` marks a reusable child template. `enabled: false` suppresses both root and child spawning. References use emitter IDs; disabled templates remain valid references.

Add `"trail": {"emitter":"smoke","interval":0.05,"count":1}` or `"death": {"emitter":"secondary","count":3}` to spawn children at a parent's current world position. A child reference spawns that template's particle directly; it does not replay the referenced template's root burst/rate schedule. Child spawn scripts receive parent velocity inputs. Trails begin one interval after birth and stop at death; death children spawn once. Children start updating on the next tick. Scripts can vary lifetime or set it to zero to expire a particle.

## Numeric Scripting
Scripts support local `let` declarations, typed assignments (`=`, `+=`, `-=`, `*=`, `/=`), `if (...) { ... } else { ... }`, arithmetic `+ - * / % ^`, comparisons, `&&`, `||`, and unary `!`. Semicolons are optional. `#` and `//` start line comments. `^` is exponentiation. There are no loops, objects, host calls, strings, arbitrary JavaScript, or dynamic property access. This is an original Joy Engine language.

Writable scalar fields: `drag size rotation spin lightIntensity lightRange lifetime u0 u1 u2 u3`. Position, velocity, acceleration, and color use the containers below. Four custom channels persist between ticks; `let` locals belong to one script invocation. Spawn/update programs have separate local scopes. All assignment targets must be declared locals or writable fields.

Named vector parameters use `type: "vector3"` and XYZ arrays for `value`, `min`, and `max`. Color parameters use `type: "color"` and four RGBA components, each constrained to 0–1, including declared bounds and runtime overrides. Scalars retain numeric bounds. Script properties are read-only and use `direction.x` or `tint.a`; whole-value reads such as `velocity = direction; color = tint;` copy their components. Existing version-1 vector suffix spellings (`directionX/Y/Z`) remain supported.

Writable containers are `position`, `velocity`, and `acceleration` (accessors `.x/.y/.z`) and `color` (accessors `.r/.g/.b/.a`). Read-only `origin` is the root or parent spawn position; `parentVelocity` is the parent velocity at child birth and zero for roots. Runtime particle views retain their flat numeric fields; these language containers compile to those fields without changing integration order or ownership. Existing version-1 flat script spellings remain supported for imported assets. Previously legal property/local names such as a scalar `velocity` retain precedence in their scope; choose a distinct property/local name to use that built-in container.

Construct values with `vector(x, y, z)` and `rgba(r, g, b, a)`. Assignments copy values, including local containers; writing a copied local does not mutate its source. Components can be assigned individually. Same-type addition/subtraction, scalar multiplication/division, and `mix(a, b, t)` work on vectors/colors. Other functions and conditions require scalars. Type mismatches and invalid component names fail at compile time with source locations. Numeric checks and the per-step budget apply to each component; constructors evaluate left to right, once per argument. RGB in scripts remains linear HDR and may exceed one; exposed color properties are limited to 0–1.

```text
let impulse = vector(0, 80, 0);
position = origin + vector(0, 10, 0);
velocity = parentVelocity + impulse;
acceleration = vector(0, -30, 0);
color = rgba(1, 0.4, 0.1, 1);
color.a = 1 - t;
```

`u0`–`u3` are four per-particle custom scalar slots initialized to zero. Values persist between spawn and update invocations; use them for initial size, a random phase, or other effect-specific state. Unlike these slots, `let` locals are recreated for each invocation.

Read-only scalar inputs: `dt age t index time pi tau` and named scalar parameters. `age` and effect `time` are seconds. `t` is normalized age/current lifetime. `index` is the instance's stable spawn sequence. During update, age advances first, then the update script executes, then velocity, position, drag and rotation integrate. Expiry and child emission follow integration. Root births follow child births in stable order.

Functions: `sin cos tan abs sqrt floor ceil round exp log`, binary `min max pow atan2`, `clamp(value,min,max)`, `mix(a,b,t)`, `smoothstep(edge0,edge1,value)`, `noise(x)`, `rand()` and `rand(min,max)`. Noise is continuous deterministic scalar value noise; it does not consume random draws. Short-circuit boolean evaluation only evaluates the required branch. Every numeric expression is checked for finite results.

## Limits and Errors
An effect permits 1–32 emitters, at most 32 parameters, 128 bursts per emitter, 4,096 live particles, 2,000 particles/second per root, and 65,536 total accepted births per reset. Each script permits 12,000 characters and 2,000 tokens, with nesting limits and a shared 2-million-expression budget per fixed tick. Population excess is dropped deterministically and counted in `droppedParticles`; death/trail ancestry stops after four child generations. Root duration and particle lifetime are at most 30 seconds; child lifetimes can extend the complete effect beyond its root duration. Trail intervals are at least one 60 Hz step. Positions and numeric fields must remain within ±1 million; particle size is 0–10,000 and drag is 0–1,000.

Compile errors retain script line/column. Runtime errors throw and set `effect.error`, stopping emission until reset; the owner should pause and report them. `update` rejects invalid frame deltas instead of hiding stalls. `seek` deterministically resets and advances to a tick at or before the requested time, bounded to 30 seconds; interactive editors should use incremental stepping for long seeks. `reset` restores original options and seed; create a new instance to apply parameter changes. `destroy` is terminal and idempotent.

The runtime simulates on the CPU and emits triangle batches through the existing GPU renderer. It does not include GPU compute simulation, oriented mesh particles, collision queries, ribbon history, dynamic property access, or middleware compatibility. Game damage/physics must remain outside cosmetic scripts.

## Particle Materials
`renderer` accepts `soft`, `streak`, `ring`, `billow`, `circle`, `square`, `textured`, and `flipbook`. Circles have a uniform filled edge; squares and PNG sprites use `size` as their world-space half extent and `rotation` as camera-facing roll. Every material multiplies its appearance by the particle's linear HDR RGB and straight alpha.

Textured and flipbook emitters require a `texture` record: `{ "source": "data:image/png;base64,...", "columns": 1, "rows": 1, "frames": 1, "fps": 0, "loop": true }`. Only embedded PNGs are accepted, making saved effects portable without external URLs. `DEFAULT_PARTICLE_TEXTURE` is an original transparent white cross for new sprites. Each PNG is limited to 1 MiB decoded file bytes and 2048×2048 pixels; an asset permits eight distinct sources and 4 MiB total encoded source text across emitters. Header validation is synchronous; resource owners must also report asynchronous PNG decoding failures.

Grid rows and columns default to one, must be integers from 1–64, and cannot exceed the image dimensions. Frames default to the grid's capacity and must be 1–columns×rows. Frames read left-to-right, top-to-bottom with half-texel UV insets to limit neighboring-cell bleed. `textured` selects the first cell; `flipbook` advances frames using particle age in seconds at `fps` (0–120). Zero FPS maps normalized particle age across all frames. `loop` defaults to true; false holds the final frame. These settings affect drawing only and never consume simulation randomness.

Use `createParticleEffectBatches(effect, camera.basis())` for mixed materials. It returns owned `{ vertices, source? }` runs in global back-to-front triangle order; consecutive triangles with the same source share a run. Colored vertices contain XYZ/RGBA (stride 7), while source-bearing runs contain XYZ/RGBA/UV (stride 9, UV origin top-left). Submit runs in order through the renderer's transparent material passes, with a loaded GPU texture for each source. The resource owner loads, caches and destroys its textures; batch generation only borrows simulation state. The original `appendParticleEffect` API remains available for colored effects and throws a clear error for textured effects rather than discarding their material.

## GPU Image Ownership
For textured effects, create `GpuTriangleRenderer` with `texturedShaderSources: PARTICLE_TEXTURE_SHADERS` and a `ParticleTextureSet(renderer)`. These shaders take an 80-byte scene uniform block (`mat4 viewProjection`, then `vec4 tint`); they output linear color and work with or without the renderer’s optional HDR post processing. Await `textures.prepare(effect.definition)` before committing a new effect, then call `textures.activate(effect.definition)` to release unused images. Failed or superseded preparation retains active images.

Call `createParticleEffectBatches(effect, cameraBasis)`, map each batch’s optional `source` to `textures.get(source)`, and pass these ordered `{vertices, texture?}` runs as the sixth `renderer.render` argument. Supply an empty fourth argument when these runs contain all translucent geometry. Sorting happens across colored and textured triangles together. PNG RGB is decoded from sRGB to linear before multiplication by the particle’s HDR tint; alpha stays straight. Destroy the texture set before the renderer.


## Attached Point Lights
An optional emitter `light` record enables one point light per living particle: `{ "intensity": 2, "range": 120 }`. Intensity must be 0–1,000 and range 0.01–10,000 world units. Each particle initializes writable `lightIntensity` and `lightRange` fields from that record; without it, both start at zero and the emitter never contributes lights, even if a script changes the fields. Scripts may animate intensity and range independently of sprite size; runtime range zero suppresses a light. Runtime values must remain within 0–1,000 intensity and 0–10,000 range.

`collectParticleLights(effect, maxLights = 32)` borrows simulation state and returns owned `{ lights, omitted }` data. Each light contains `{ x, y, z, r, g, b, range, emitterIndex, particleIndex }`. Position follows the particle, RGB is nonnegative linear HDR multiplied by intensity and clamped particle alpha, and range is a world-space radius. Zero intensity, zero range, dark RGB, invisible alpha, expired particles and invalid values contribute no light. Sprite size does not control light visibility. The function selects up to 32 strongest emitted luminances (stable birth index breaks ties), reports remaining eligible lights as `omitted`, and consumes no randomness. Owners decide which scene surfaces receive these lights; collecting lights never advances simulation or creates GPU resources.

## CPU Performance Inspection
Pass `{ profiling: true }` to `ParticleEffect` to enable the instance-owned `.profile` record. Its `emitters` array follows definition order and contains `{ id, spawnMs, updateMs, spawned, dropped, live, operations }`, with top-level `enabled`, `steps` and `simulationMs`. Profiling defaults off and does not read a clock when disabled. An optional `profileClock` returns monotonic milliseconds for deterministic instrumentation tests. Metrics never feed simulation, scheduling, budgets or seeded randomness.

Times and counters are cumulative since reset. `reset` clears records in place, then includes initialization and time-zero births in CPU time and spawn counters; `seek` resets and measures the subsequent replay. `steps` counts attempted fixed steps, including a failing step. `spawnMs` measures accepted-particle construction, spawn scripts and capacity handling; `updateMs` measures each emitter's script, integration and child scheduling, while child construction is attributed to the child's spawn row. `simulationMs` measures complete reset/step work, including shared scheduling and instrumentation overhead. Per-emitter totals exclude some shared overhead and are not GPU timings. Owners should subtract successive snapshots for frame/window costs.

`spawned` counts accepted birth attempts (including a birth whose script fails), `dropped` counts deterministic capacity rejection for that emitter, and `operations` counts actual script expression-budget consumption, including failed work. `live` counts unexpired particles and refreshes after reset, each step, errors and destroy; destroy preserves cumulative counters while clearing live counts. Disabled emitters produce neither births nor drops.

For rendering costs, create an owned record with `createParticleRenderMetrics(definition)` and pass it as the optional third argument of `createParticleEffectBatches(effect, cameraBasis, metrics)`. Every build resets its `emitters` rows `{ id, triangles, vertices, geometryMs }` and shared `sortMs`. Geometry time includes CPU billboard construction and depth/triangle preparation; shared sort time includes global sorting and material-run assembly. Triangle and vertex totals describe submitted geometry rather than draw calls. These measurements are optional CPU diagnostics, carry no GPU-time or per-emitter draw-call attribution, and do not change batch output.

PARTICLE LAB's Monitor selector defaults to Overview: aggregate simulation/geometry/submission CPU timings and geometry/light counts. Detailed profiling opts into per-emitter simulation and geometry clocks; leaving Detailed releases render metrics and stops those clocks without changing particle state. Disabled skips monitoring clocks, per-frame stat snapshots and monitoring DOM updates. The displayed scene batch count describes nonempty scene geometry batches, excludes postprocessing and is not observed backend draws. Vertex payload bytes exclude uniforms, textures and driver overhead. CPU submission time is not GPU execution time. Per-emitter totals begin when Detailed is enabled and reset on restart, seek or loop.

## Game-Owned Streams and Lifecycle Operations
`ParticleEffectOptions.random` optionally borrows a `() => number` stream returning values in [0, 1). Use it when an existing game owns cosmetic sequencing. `reset` and `seek` continue this external stream; they cannot rewind it. Omit `random` for seed-reproducible editor replay. Definitions remain immutable and can be shared by either mode.

`removeOldestParticles(count)` removes up to that many live particles without firing death emitters, changing elapsed time, or rewinding the spawn index. SPACE SCUFFLES uses it to enforce its session-wide exhaust budget without evicting explosions. `scalePositions({x, y, z})` rescales current and future spawn positions around world zero with positive finite factors, leaving size, velocity, forces, and timing unchanged. BLAST GARDEN calls it when the garden's aspect ratio changes. Destroy each instance when it expires, when the session resets, and at application disposal.

BLAST GARDEN currently preserves its shared round/debris stream and the prior random draw order. Separating those streams is tracked in [CAL-36](https://linear.app/calvinball/issue/CAL-36/separate-garden-cosmetic-randomness-from-round-generation-without).

## Asset Filenames
Particle documents use the `.joyfx` extension and contain JSON. Import checked-in assets with Vite's `?raw` suffix, then call `JSON.parse` before `compileParticleEffect`. The shared Node test loader supports the same raw imports. PARTICLE LAB opens and saves this format directly.
