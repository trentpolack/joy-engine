# Scripted Particle Effects
Joy Engine's Rust particle runtime compiles reusable `.joyfx` definitions and creates independently owned `ParticleEffect` instances. Rendering borrows simulation state; games own cosmetic recipes, lifetime, and event mapping. See [runtime integration](rust-particles.md) for Rust examples, texture ownership, lights, and staged level effects.

Particles use XYZ world space, seconds, velocity in units/second, acceleration in units/second², and radians. RGB is linear HDR; alpha is clamped for rendering. Use camera-facing geometry for orbit views or `PARTICLE_XY_BASIS` for fixed arcade presentation. Cosmetic scripts never own damage or physics.

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

Add `"light": {"intensity":2,"range":6}` to make an emitter's particles point lights; scripts vary them through `lightIntensity` and `lightRange`. Particle lights are ordinary point lights to the renderer: they rank with level lights for the view, and `"castsShadow": true` opts an emitter into soft point shadows (off by default, since particle lights are numerous and short-lived and each shadowed light renders six shadow faces).

Add `"trail": {"emitter":"smoke","interval":0.05,"count":1}` or `"death": {"emitter":"secondary","count":3}` to spawn children at a parent's current world position. A child reference spawns that template's particle directly; it does not replay the referenced template's root burst/rate schedule. Child spawn scripts receive parent velocity inputs. Trails begin one interval after birth and stop at death; death children spawn once. Children start updating on the next tick. Scripts can vary lifetime or set it to zero to expire a particle.

## Numeric Scripting
Scripts support local `let` declarations, typed assignments (`=`, `+=`, `-=`, `*=`, `/=`), `if (...) { ... } else { ... }`, arithmetic `+ - * / % ^`, comparisons, `&&`, `||`, and unary `!`. Semicolons are optional. `#` and `//` start line comments. `^` is exponentiation. There are no loops, objects, host calls, strings, arbitrary host code, or dynamic property access. This is an original Joy Engine language.

Writable scalar fields: `drag size rotation spin lightIntensity lightRange lifetime u0 u1 u2 u3`. Position, velocity, acceleration, and color use the containers below. Four custom channels persist between ticks; `let` locals belong to one script invocation. Spawn/update programs have separate local scopes. All assignment targets must be declared locals or writable fields.

Named vector parameters use `type: "vector3"` and XYZ arrays for `value`, `min`, and `max`. Color parameters use `type: "color"` and four linear RGBA components: RGB is nonnegative and may exceed one for HDR tints (up to 1,000,000), and alpha stays within 0–1, for values, declared bounds, and runtime overrides alike. Scalars retain numeric bounds. Script properties are read-only and use `direction.x` or `tint.a`; whole-value reads such as `velocity = direction; color = tint;` copy their components. Existing version-1 vector suffix spellings (`directionX/Y/Z`) remain supported.

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

Compile errors retain script line/column. Runtime errors return typed errors and stop the effect, stopping emission until reset; the owner should pause and report them. `update` rejects invalid frame deltas instead of hiding stalls. `seek` deterministically resets and advances to a tick at or before the requested time, bounded to 30 seconds; interactive editors should use incremental stepping for long seeks. `reset` restores original options and seed; create a new instance to apply parameter changes. Release effects when their owning session ends.

The runtime simulates on the CPU and emits triangle batches through the existing GPU renderer. It does not include GPU compute simulation, oriented mesh particles, collision queries, ribbon history, dynamic property access, or middleware compatibility. Game damage/physics must remain outside cosmetic scripts.

## Particle Materials
`renderer` accepts `soft`, `streak`, `ring`, `billow`, `circle`, `square`, `textured`, and `flipbook`. Circles have a uniform filled edge; squares and PNG sprites use `size` as their world-space half extent and `rotation` as camera-facing roll. Every material multiplies its appearance by the particle's linear HDR RGB and straight alpha.

Textured and flipbook emitters require a `texture` record: `{ "source": "data:image/png;base64,...", "columns": 1, "rows": 1, "frames": 1, "fps": 0, "loop": true }`. Only embedded PNGs are accepted, making saved effects portable without external URLs. `DEFAULT_PARTICLE_TEXTURE` is an original transparent white cross for new sprites. Each PNG is limited to 1 MiB decoded file bytes and 2048×2048 pixels; an asset permits eight distinct sources and 4 MiB total encoded source text across emitters. Header validation is synchronous; resource owners must also report PNG decoding failures.

Grid rows and columns default to one, must be integers from 1–64, and cannot exceed the image dimensions. Frames default to the grid's capacity and must be 1–columns×rows. Frames read left-to-right, top-to-bottom with half-texel UV insets to limit neighboring-cell bleed. `textured` selects the first cell; `flipbook` advances frames using particle age in seconds at `fps` (0–120). Zero FPS maps normalized particle age across all frames. `loop` defaults to true; false holds the final frame. These settings affect drawing only and never consume simulation randomness.

Use `create_particle_effect_batches` for mixed materials. It returns owned `{ vertices, source? }` runs in global back-to-front triangle order; consecutive triangles with the same source share a run. Colored vertices contain XYZ/RGBA (stride 7), while source-bearing runs contain XYZ/RGBA/UV (stride 9, UV origin top-left). Submit runs in order through the renderer's transparent material passes, with a loaded GPU texture for each source. The resource owner loads, caches and destroys its textures; batch generation only borrows simulation state. The colored-triangle API `append_particle_effect` remains available for colored effects and returns an error for textured effects rather than discarding their material.


## Files and Authoring
`.joyfx` files contain JSON. Embed or load their text, then call `parse_particle_effect`; use `compile_particle_effect` for already-parsed documents. PARTICLE LAB edits the same format that games consume. The renderer owns GPU texture resources; project effects own simulation state and must be released or reset with the session.
