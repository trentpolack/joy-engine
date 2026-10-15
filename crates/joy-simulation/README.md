# joy-simulation
The headless simulation layer of Joy Engine's Rust workspace: the environment clock, rigid-body physics and raycast vehicles, entities and controllers, authored level and object documents, and the level runtime. It is a port of the TypeScript engine's `src/environment`, `src/physics`, and `src/gameplay` simulation code. It depends only on `joy-foundation`, never on rendering or platform crates, and builds identically for native targets and `wasm32-unknown-unknown`.

## Module Map
| Module | Contents | TypeScript source |
| --- | --- | --- |
| `environment` | `EnvironmentOptions` (serde, TypeScript spellings), `EnvironmentSystem`, `EnvironmentFrame` | `environment/environment-system.ts` |
| `physics` | `PhysicsWorld`, `BodyOptions`/`BODY_DEFAULTS`, `VehicleOptions`/`WheelOptions`/`VEHICLE_DEFAULTS`/`WHEEL_DEFAULTS`, `BodyHandle`, `VehicleHandle`, `RaycastHit` | `physics/*.ts` |
| `gameplay` (documents) | `EntityDefinition`, `EntityTransform`, `LevelData`, `parse_level`/`serialize_level`, `ObjectDocument`, `resolve_level_objects`, `asset_path` | `entity-definition.ts`, `level-document.ts`, `object-document.ts`, `asset-path.ts` |
| `gameplay` (components) | `component_type`, `entity_type`, `RENDER_/LIGHT_/EFFECT_COMPONENT_DEFAULTS`, `RenderComponent::read`, `LightComponent::read` | `constants.ts`, `built-in-component-schemas.ts`, `level-scene.ts` |
| `gameplay` (runtime) | `EntityId`, `Entity`/`EntityState`, `EntityBehavior`, `SimulationContext`, `EntityRegistry`, `EntityWorld`, `EntitySystem`, `ControllerId`, `IntentController`, `World`, `LevelRuntime` | `entity*.ts`, `character.ts`, `controller.ts`, `player-controller.ts`, `world.ts`, `level-runtime.ts` |
| `gameplay::grid_navigation` | `search_grid`, `trace_grid_route` | `grid-navigation.ts` |

## Ownership Model
Owners hand out copyable ids and handles instead of references. `PhysicsWorld` issues `BodyHandle` and `VehicleHandle` values stamped with the issuing world's identity, so a handle from another world or a removed body is reported as unknown rather than aliasing a different body. `EntityWorld` owns entities (insertion-ordered) and controllers and issues `EntityId` and `ControllerId` values.

Each `Entity` is split into plain `EntityState` and an optional boxed `EntityBehavior`. The world takes the behavior out while it runs, so hooks receive `&mut EntityState` plus a `SimulationContext` (a mutable borrow of physics and read access to the entity-to-body and entity-to-vehicle maps) without aliasing their owner. Structural changes from hooks go through `SimulationContext::request_spawn`/`request_remove`. Bodies and vehicles listed for an entity are owned by it (the TypeScript `entity.own`): removing the entity removes them. `World` is the composition root that owns `EntityWorld`, `PhysicsWorld`, and the handle maps, and `World::split` provides the borrows; dropping it removes entities (running `on_destroy`) before physics.

## Physics Backend
Physics uses rapier3d 0.36 with `enhanced-determinism`; identical worlds stepped identically produce bit-identical transforms, and rapier types never appear in the public API. Inputs that feed the solver (Euler rotations and damping coefficients) are computed with rapier's libm-backed scalar math rather than platform math, so native and WebAssembly builds create bit-identical bodies. Stepping keeps the TypeScript accumulator exactly (accumulate, clamp to `fixed_step * max_sub_steps`, step while `accumulator + 1e-12 >= fixed_step`, discard the excess), and each fixed step updates vehicles, steps the solver, and then clears one-step user forces.

Mapping from cannon-es (the TypeScript backend):
- Mass 0 is a fixed body; planes face +Y before rotation and become rapier half-spaces.
- Friction and restitution combine by multiplication (`CoefficientCombineRule::Multiply`), like cannon's material product.
- Damping `d` is cannon's per-second fraction (`v *= (1 - d)^dt`); rapier uses `v *= 1 / (1 + dt * c)`, so bodies use `c = -ln(1 - d)`, with `d = 1` mapped to a large finite coefficient.
- Bodies never sleep (cannon worlds default to `allowSleep = false`).
- Raycasts test every body against its current pose (so new or teleported bodies are found before the next step) and ignore a body whose shape contains the ray start, like cannon's `skipBackfaces`.
- Vehicles use rapier's `DynamicRayCastVehicleController` (Bullet-derived, like cannon's `RaycastVehicle`) with up = +Y, forward = +Z, ray (0, -1, 0), and axle (-1, 0, 0). Positive throttle drives +Z directly (cannon's tire tangent was -Z, hence the TypeScript negation), positive steer turns toward +X, and wheel spin is reported with cannon's sign so wheels visibly roll forward.

Known gaps against cannon-es:
- `rollInfluence` is validated and stored, but rapier 0.36 keeps it private at 0.1 (the TypeScript default is 0.01).
- Rapier divides rolling and brake friction by the number of grounded wheels before clamping to `brake_force`; braking at the bound matches, gentle rolling resistance is weaker.
- Rapier does not apply the opposite side impulse to a dynamic ground body.
- Wheel rays query rapier's broad phase, which includes a new or moved collider only after the next solver step, so a vehicle created on the same frame as its ground lands one fixed step later.
- Solver details (iterations, contact softness) differ, so trajectories are close but not identical. In the TypeScript suite's drive scenario both engines travel 17.8 m in two seconds at 17.6 m/s.

## TypeScript Semantics
Preserved: defaults, validation ranges, time units, update order (entities in insertion order; `World` steps physics then entities; `LevelRuntime` updates entities with `min(seconds, 0.1)`, steps physics, then copies body poses into entities), exclusive possession with intent clearing and reset hooks on both sides, entities spawned during an update waiting for the next one, component scaling and rollback, document key order and number formatting (levels round trip byte for byte), and grid-search tie-breaking and settle order.

Intentional differences:
- Every entity can be possessed; there is no `Character` subclass. Removing any entity clears its intent and calls `reset_control`.
- `EntitySystem` matches authored component records; the runtime `ComponentRegistry`/`EntityComponent` layer is not ported.
- Spawn and remove requests made from `on_spawn`/`on_destroy` apply at the start of the next update instead of synchronously. Behavior `update` hooks and controller samplers cannot fail.
- `EntityRegistry::register` returns `Result` (duplicate and blank types fail, like the TypeScript throws); `register_plain` registers a type without behavior. Unregistered types fail to spawn.
- `EntityTransform` stores `f32`; authored transform values needing double precision are rounded to the nearest `f32` and written back in its shortest form.
- `serialize_level` writes as-is (call `LevelData::validate` for edited data); `serialize_object` validates and returns `Result`.
- `World` and `LevelRuntime` own their physics world (the TypeScript borrowed-solver option becomes `World::with_physics`/`LevelRuntime::with_physics_options`).
- Queries on removed bodies or vehicles return `None` (or zero/empty) instead of throwing; `raycast` returns `None` for non-finite input.
- The editor-only `LevelDocument` undo history, component schema catalog, `LevelScene`, and `AssetScene` are outside this crate.
