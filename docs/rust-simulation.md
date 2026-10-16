# Rust Simulation
The headless simulation layer of Joy Engine's Rust crate (`src/environment`, `src/physics`, and `src/gameplay`): the environment clock, rigid-body physics and raycast vehicles, destructible chunk structures, entities, runtime components, systems, and controllers, authored level and object documents with the editor's undo history, the component schema catalog, and the level runtime. It uses only the foundation layer (`src/core`), never content, rendering, or platform modules, and builds identically for native targets and `wasm32-unknown-unknown`.

## Module Map
| Module | Contents |
| --- | --- |
| `environment` | `EnvironmentOptions` (serde), `EnvironmentSystem`, `EnvironmentFrame` |
| `physics` | `PhysicsWorld`, `BodyOptions`/`BODY_DEFAULTS`, `VehicleOptions`/`WheelOptions`/`VEHICLE_DEFAULTS`/`WHEEL_DEFAULTS`, `BodyHandle`, `VehicleHandle`, `RaycastHit` |
| `physics` (backend contract) | `PhysicsBackend` (implemented by `PhysicsWorld`), `PhysicsResidency`, `PhysicsCollisionShape`, `PhysicsMutationBatch`, `PhysicsQueryHit`, `create_removal_mutation_batch` |
| `physics::destruction` | `ChunkStructure`, `DestructibleChunkDefinition`, `RadialDamage`, `ChunkChangeSet`, `ChunkDamageState`, `ChunkRaycastHit`, `ChunkSupportOptions`, `ChunkSupportState`, `create_chunk_mesh_data`, `DestructionError` |
| `gameplay` (documents) | `EntityDefinition`, `EntityTransform`, `LevelData`, `parse_level`/`serialize_level`, `LevelDocument`, `ObjectDocument`, `resolve_level_objects`, `asset_path` |
| `gameplay` (components) | `component_type`, `entity_type`, `component_preset`, `component_category`, `component_asset_kind`, `RENDER_/LIGHT_/EFFECT_COMPONENT_DEFAULTS`, `RenderComponent::read`, `LightComponent::read` |
| `gameplay` (schemas) | `ComponentSchemaCatalog`, `ComponentSchema`, `ComponentFieldSchema`, `ComponentFieldKind`, `ComponentFieldCondition`, `ComponentPreset`, `ComponentIssue`, `CreatedComponent`, `is_component_field_active`, `built_in_component_schemas`, `ComponentSchemaError` |
| `gameplay` (runtime) | `EntityId`, `Entity`/`EntityState`, `EntityBehavior`, `EntityComponent`, `ComponentRegistry`, `SimulationContext`, `EntityRegistry`, `EntityWorld`, `EntitySystem`, `World`, `LevelRuntime` |
| `gameplay` (control) | `ControlIntent`, `ControlValue`, `Controller`, `IntentController` (with `PlayerController` and `AiController` aliases), `ControllerId` |
| `gameplay::grid_navigation` | `search_grid`, `trace_grid_route` |

Simulation also uses these foundation additions in `src/core`: `config_values` (`ConfigField`, `ConfigApplyMode`, `create_config_values`, `create_config_document_values`, `create_config_schema`), `types` (`Vector2`, `Velocity2`, `MovingBody`, `Vector3`, `AffineTransform3D`, `Color`, `IndexedMesh3D`, `ShadedTriangle3D`, `ShadedMesh3D`, `MeshAssetData`, `GeometryBounds`, `MaterialAlphaMode`, `PbrMaterial`, `PbrMesh`), `math` (`distance_between`, `direction_between`, `js_hypot`), and `color` (`parse_hex_color` and `with_alpha` on the `f64` `Color`). Foundation records use `f64`; `core::Rgb`/`Rgba` remain the `f32` GPU colors.

## Ownership Model
Owners hand out copyable ids and handles instead of references. `PhysicsWorld` issues `BodyHandle` and `VehicleHandle` values stamped with the issuing world's identity, so a handle from another world or a removed body is reported as unknown rather than aliasing a different body. `EntityWorld` owns entities (insertion-ordered), controllers, and systems, and issues `EntityId`, `ControllerId`, and `SystemId` values.

Each `Entity` is split into plain `EntityState`, an optional boxed `EntityBehavior`, and runtime `EntityComponent`s keyed by component type. The world takes the behavior out while it runs, and borrows a component and the state as disjoint fields, so hooks receive `&mut EntityState` plus a `SimulationContext` (a mutable borrow of physics and read access to the entity-to-body and entity-to-vehicle maps) without aliasing their owner. Structural changes from hooks go through `SimulationContext::request_spawn`/`request_remove`. Bodies and vehicles listed for an entity are owned by it: removing the entity removes them. Component and behavior resources are Rust values released by `Drop`.

Spawning registers the entity, attaches a runtime component for every authored component type the `ComponentRegistry` knows (in authored order), and then runs `on_spawn`; a failing `on_attach` detaches that component (running `on_detach`) and rolls back the whole spawn. Removal releases possession, runs `on_destroy`, detaches components in reverse attachment order (each `on_detach`, then drop), removes the entity's bodies and vehicles, and finally drops the behavior. Systems run before behaviors in insertion order and receive `&mut [&mut Entity]` matched by runtime components; `on_add` failures remove the system again (running `on_remove`). `World` is the composition root that owns `EntityWorld`, `PhysicsWorld`, and the handle maps (`World::split` provides the borrows, `World::from_parts` accepts an `EntityWorld` built with a component registry); dropping it removes systems (reverse order), then entities, then physics.

`ChunkStructure` copies its definitions and owns all damage state; queries borrow definitions, and change sets, damage states, and support diagnostics are owned copies. `LevelDocument` owns its level, up to 80 undo snapshots, the redo stack, and the selection; edits run on isolated drafts and `snapshot()` returns owned copies. `ComponentSchemaCatalog` owns registered schemas and lends read-only borrows.

## Physics Backend
Physics uses rapier3d 0.36 with `enhanced-determinism`; identical worlds stepped identically produce bit-identical transforms, and rapier types never appear in the public API. Inputs that feed the solver (Euler rotations and damping coefficients) are computed with rapier's libm-backed scalar math rather than platform math, so native and WebAssembly builds create bit-identical bodies. Stepping uses a bounded accumulator (accumulate, clamp to `fixed_step * max_sub_steps`, step while `accumulator + 1e-12 >= fixed_step`, discard the excess), and each fixed step updates vehicles, steps the solver, and then clears one-step user forces.

`PhysicsWorld` implements the `PhysicsBackend` contract as the engine's CPU backend: `apply_mutations` removes listed shape ids and adds static boxes keyed by shape id, atomically, rejecting batches older than the newest applied sequence, duplicate or empty ids, and non-finite or non-positive boxes; `query` casts only against resident shapes and reports world distance; `step` advances the solver. Destructible structures feed it through `create_removal_mutation_batch(change.revision, removed_ids)`, so collision changes at the same boundary as damage, and debris bodies collide with the surviving chunks.

## Physics Conventions and Limits
- Mass zero is fixed; unrotated planes face +Y. Friction and restitution combine by multiplication.
- Damping is an authored per-second fraction translated into the solver coefficient. Bodies do not sleep.
- Raycasts inspect current poses and ignore a body containing the ray origin.
- Vehicles use Y-up, +Z forward, downward suspension rays, and axle (-1, 0, 0). Positive throttle drives forward; positive steering turns toward +X.
- `rollInfluence` is validated and stored, but rapier 0.36 keeps its internal value private at 0.1.
- Rolling/brake friction is divided by grounded-wheel count before clamping. Opposite side impulses are not applied to dynamic ground bodies.
- Wheel ray queries see new/moved colliders after the next solver step. Discrete solver behavior and authored tuning can produce wheelies or tunneling; test driving interactively.

## Destruction
`ChunkStructure` is solver-independent and uses `f64` arithmetic and portable distance helpers, preserving recorded deterministic results. Damage is queued and applied atomically at `commit()`: overlapping hits accumulate, chunks at zero health leave first, then support is evaluated once and unsupported chunks detach in the same revision (the support diagnostics describe that evaluation). Support is a geometric masonry rule: chunks touching the ground plane anchor, stacked contacts and side-face contacts (never corners) carry support, and horizontal offsets spend the cantilever allowance; a shortest-path search with generation-order tie-breaking resolves reach. Revisions increment only when chunks leave and on every `reset()`. `create_chunk_mesh_data` returns closed boxes as plain `MeshAssetData` (positions, indices, per-vertex material tints, and `chunkCount`/`closedInteriorSurfaces` metadata) for the rendering layer to upload.

## Runtime Contracts and Verification
Entities update in insertion order. `World` steps physics before entities; `LevelRuntime` updates entities with at most 0.1 seconds, steps physics, then copies poses. Component setup rolls back on failure, possession is exclusive, and removal clears control intent. Spawn/remove requests from lifecycle hooks apply at the next update boundary.

`LevelDocument` validates isolated drafts and keeps up to 80 undo snapshots. `EntityTransform` stores `f32`; authored transforms are rounded at that boundary. `serialize_level` writes the supplied data, so validate edited levels explicitly; object serialization validates and returns `Result`.

Registries reject duplicate/blank types. Runtime component setup can fail in `on_attach`; factories themselves are infallible. Systems cannot be structurally changed during their update. Worlds own their physics. Removed body/vehicle queries return `None` or empty results, and nonfinite ray input is rejected. Mutation sequences are unsigned and backend operations synchronous.

Recorded fixtures cover destruction revisions, support/raycast/mesh results, schema defaults and validation, document edit/undo/redo serialization, grid tie-breaking, and component/system lifecycle order. `test/rust/` runs those scenarios natively; WebAssembly checks ensure the same headless code builds for browsers.
