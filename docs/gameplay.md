# Gameplay, Levels, and Physics
The headless gameplay layer separates authored data, runtime ownership, and presentation. `EntityState` holds identity, transforms, tags, components, and intent; `EntityBehavior` supplies project behavior. `EntityWorld` owns entities, controllers, and systems, while `World` composes those owners with physics.

| Owner | Responsibility |
| --- | --- |
| `ComponentRegistry` | Resolve trusted component types to runtime behavior. |
| `ComponentSchemaCatalog` | Describe fields, defaults, units, validation, and editor presets. |
| `Controller` / `IntentController` | Exclusive possession and player/AI intent sampling. |
| `EntityRegistry` | Resolve trusted entity types to factories. |
| `LevelDocument` | Validated authored edits, selection, and bounded undo/redo. |
| `LevelRuntime` | Instantiate copied level data and physics. |
| `LevelScene` / `AssetScene` | Present meshes, lights, and effects in the rendering layer. |
| `PhysicsSimulation` | Fixed-step rigid bodies and ray queries. |

Unknown authored components remain data; only registered runtime behavior runs. Resource acquisition and rollback follow component/entity lifecycle hooks. Spawn/removal requests from hooks apply at update boundaries. Possession changes clear old intent, and removing an entity releases its body.

## Authored Data
`.joylevel` and `.joyobject` are versioned JSON documents. Entity IDs are stable and unique; component records retain project-specific data. Coordinates are Y up and +Z forward, distances are meters where the level contract specifies them, and rotations are XYZ Euler radians. Runtime transforms are copies and never write back into the authoring document.

## AI Building Blocks
`gameplay::ai` holds the deterministic pieces every game's AI shares, independent of the entity world: `BestChoice` keeps the first strictly better candidate (so equal scores resolve to enumeration order) with an optional bound and a lazily evaluated acceptance test for costly checks such as line of sight; `nearest` and `SightCone` answer range and cone perception over caller-filtered 2D or 3D positions; and the steering helpers turn headings, planar velocities, and 3D directions by explicit per-call limits, with `home_direction` losing targets outside a seeker cone. Games keep factions, scoring rules, and decisions; Lua scripts reach the angle and distance helpers as engine globals.

`LevelDocument::transact` edits an isolated draft and validates before acceptance. `ComponentSchemaCatalog` provides advisory issues and field metadata for inspectors; runtimes validate their own boundaries. `search_grid` and `trace_grid_route` provide deterministic weighted navigation with project-owned costs and occupancy.

## Physics and Destruction
Rapier provides rigid bodies through engine-owned types. Keep barriers thick enough and speeds appropriate for the discrete solver. See [simulation and physics](rust-simulation.md) for determinism, ownership, and solver limits.

Bodies are boxes, spheres, or planes by default. `SdfShape` colliders give non-convex procedural geometry (FORM meshes baked to distance grids, or analytical shapes) its true silhouette in collision; see [SDF colliders](rust-simulation.md#sdf-colliders).

`ChunkStructure` batches damage and support evaluation into a single committed revision; the renderer and physics backend synchronize from that revision. Optional stress gives chunks load paths: weakened or undermined chunks overload, telegraph through a failure delay, and then fail, so collapse can be planned, baited, and fled. A sampled ground surface lets craters undermine what stands on them. `DebrisWorld` runs the debris itself: per-structure colliders, the budgeted rigid pool, and kinetic-energy strikes against upright-cylinder targets. Project-specific collapse, damage, and spawn rules stay in the game; see [destruction](rust-simulation.md#destruction).

## Authoring
Run Joy Editor natively, open a project, and edit its assets. Saves use revision checks and recovery copies. A static browser editor offers bundled examples and downloads; it cannot write project files. Run games through their project commands and restart/rebuild to consume saved assets where the game does not expose live loading. See [Joy Editor](../editor/README.md).
