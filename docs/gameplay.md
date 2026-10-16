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
| `PhysicsWorld` | Fixed-step rigid bodies, ray queries, and raycast vehicles. |

Unknown authored components remain data; only registered runtime behavior runs. Resource acquisition and rollback follow component/entity lifecycle hooks. Spawn/removal requests from hooks apply at update boundaries. Possession changes clear old intent, and removing an entity releases its bodies and vehicles.

## Authored Data
`.joylevel` and `.joyobject` are versioned JSON documents. Entity IDs are stable and unique; component records retain project-specific data. Coordinates are Y up and +Z forward, distances are meters where the level contract specifies them, and rotations are XYZ Euler radians. Runtime transforms are copies and never write back into the authoring document.

`LevelDocument::transact` edits an isolated draft and validates before acceptance. `ComponentSchemaCatalog` provides advisory issues and field metadata for inspectors; runtimes validate their own boundaries. `search_grid` and `trace_grid_route` provide deterministic weighted navigation with project-owned costs and occupancy.

## Physics and Destruction
Rapier provides rigid bodies and configurable raycast vehicles through engine-owned types. Vehicle tires are suspension rays rather than solid rolling colliders. Keep barriers thick enough and speeds appropriate for the discrete solver. See [simulation and physics](rust-simulation.md) for determinism, ownership, and vehicle limits.

`ChunkStructure` batches damage and support evaluation into a single committed revision; the renderer and physics backend synchronize from that revision. Project-specific collapse, damage, and debris rules stay in the game.

## Authoring
Run Joy Editor natively, open a project, and edit its assets. Saves use revision checks and recovery copies. A static browser editor offers bundled examples and downloads; it cannot write project files. Run games through their project commands and restart/rebuild to consume saved assets where the game does not expose live loading. See [Joy Editor](../editor/README.md).
