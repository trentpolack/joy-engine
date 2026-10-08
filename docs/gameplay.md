# Gameplay, levels and physics
[Engine guide](README.md) · [API reference](api/README.md)

Joy's gameplay layer separates the object being controlled from the source of its intent. `Entity` is the base form for anything placed in a world: it provides stable identity, a world-space transform, tags, authored component records, runtime components and explicit cleanup. Projects can use subclasses for cohesive actor-like behavior, compose behavior through components and systems, or combine both while migrating existing code.

## Responsibilities
| Owner | Responsibility |
| --- | --- |
| `Entity` | Base world object with identity, transform, tags, component data and owned resource cleanup. |
| `EntityComponent` | One entity-owned behavior over an authored component record. |
| `ComponentRegistry` | Resolve trusted component names to runtime component factories. |
| `EntitySystem` | Process a stable query of entities with a required component composition. |
| `Character` / `PlayerCharacter` | Possessable entity and project gameplay response to control intent. |
| `Controller` | Exclusive possession, switching and unpossessing. |
| `PlayerController` / `AIController` | Sample player input or AI decisions into the same intent record. |
| `EntityRegistry` | Resolve trusted type names to project factories. |
| `EntityWorld` | Spawn, update and remove entities; roll back failed setup. |
| `LevelDocument` | Validate authored levels and retain bounded undo/redo history. |
| `LevelRuntime` | Instantiate entities and physics from a copied level. |
| `LevelScene` | Synchronize primitive presentation with retained mesh instances. |
| `PhysicsWorld` | Rigid bodies, collision queries, fixed stepping and vehicle lifetimes. |

Game rules stay in project character subclasses. A controller may outlive a character and possess another one. Transfer and destruction clear intent and detach both sides. `PlayerController` borrows a sampler; the application still owns input listeners, camera policy, menus and pause state. No input device is hard-coded into a character.

```js
import { PlayerCharacter, PlayerController, EntityRegistry, EntityWorld } from 'joy-engine';

class Hero extends PlayerCharacter {
  update(seconds) {
    // Interpret this.controlIntent here; abilities and movement belong to the character.
  }
}

const registry = new EntityRegistry();
registry.register('hero', definition => new Hero(definition));
const world = new EntityWorld({registry});
const hero = world.spawn({id: 'hero-1', type: 'hero'});
const controller = new PlayerController({sampleInput: () => ({moveX: 0, moveZ: 1})});
controller.possess(hero);
// Each simulation frame: controller.update(seconds), then world.update(seconds).
// Teardown: controller.destroy(), then world.destroy().
```

Entity factories construct data. Acquire resources in `onSpawn(world)` and attach them with `this.own(resource)`; cleanup runs in reverse acquisition order, including failed spawn. Prefer lifecycle hooks over replacing `destroy()`; the base destruction path protects possession and owned resources. Worlds and registries have no mutable global instance.

## Component and system model
Authored `components` remain portable JSON data. A `ComponentRegistry` optionally maps any of those names to runtime `EntityComponent` behavior. During spawn, the world registers the entity first, constructs each known runtime component in authored order, attaches it, and finally calls the entity's `onSpawn`. Unknown component records remain valid data for render, physics, editor or later project adapters; a project does not need a runtime class for every record.

```js
import { ComponentRegistry, EntityComponent, EntitySystem, EntityWorld } from 'joy-engine';

class HealthComponent extends EntityComponent {
  constructor(data) {
    super('health', data);
  }
}

class RegenerationSystem extends EntitySystem {
  constructor() {
    super({requiredComponents: ['health']});
  }

  update(seconds, entities) {
    for(const entity of entities) {
      const health = entity.getComponent('health');
      health.data.current = Math.min(health.data.maximum, health.data.current + health.data.perSecond*seconds);
    }
  }
}

const componentRegistry = new ComponentRegistry()
  .register('health', data => new HealthComponent(data));
const world = new EntityWorld({componentRegistry});
world.addSystem(new RegenerationSystem());
world.spawn({
  id: 'villager-1',
  components: {health: {current: 80, maximum: 100, perSecond: 2}}
});
```

Components borrow their entity-owned JSON record rather than copying it again, so editor/runtime adapters and systems observe one value. `onAttach(entity)` may acquire resources with `own`; `onDetach()` and owned resources run during removal or failed-spawn rollback. Systems are world-owned and run in insertion order before legacy `Entity.update` hooks. Each update uses a stable snapshot: removal takes effect immediately, while entities spawned by a system wait until the next frame. `query(componentTypes)` returns a borrowed-entity snapshot and never transfers ownership.

This first version deliberately avoids archetype storage, implicit global registries and parallel scheduling. The object-oriented lifetime shell keeps browser, GPU and physics ownership explicit; data-oriented systems provide the flexible composition point. Hot paths can later move component data into specialized storage without changing authored level documents or the base world-object contract.

## Authored data
A `.joylevel` file uses `{version:1,name,entities:[...]}`. Each entity has a stable `id`, trusted registered `type`, display `name`, `tags`, `transform`, and `components`. IDs must be unique. Components are JSON-compatible records, so project-specific fields round-trip through the editor. They cannot contain executable scripts or arbitrary factory references.

Coordinates are Y up, +Z forward, meters; rotations are [x, y, z] radians applied X, then Y, then Z (Rz × Ry × Rx), and elapsed time is seconds. Transforms are currently world-space. `render.size` supplies full primitive dimensions and is multiplied by `transform.scale`. Collider dimensions are local and are also scaled at instantiation. Use matching render/collider dimensions. Simulation transforms are copied from authored data, then owned by runtime entities.

```json
{
  "version": 1,
  "name": "Small playground",
  "entities": [{
    "id": "crate-1",
    "type": "entity",
    "name": "Crate",
    "tags": ["prop"],
    "transform": {"position": [0, 2, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1]},
    "components": {
      "render": {"shape": "box", "color": [1, 0.25, 0.04]},
      "physics": {"shape": "box", "mass": 8, "halfExtents": [0.5, 0.5, 0.5]}
    }
  }]
}
```

`parseLevel` validates the serialization boundary; `serializeLevel` returns readable JSON. `LevelDocument.transact` edits a copied draft, validates it, and only then replaces authored state. `snapshot()` provides an independent runtime copy. Unsupported versions, duplicate IDs and invalid values fail visibly.

## Vehicle mechanism
`PhysicsWorld` wraps the MIT-licensed `cannon-es` solver. It supports static and dynamic boxes/spheres, static planes, forces, impulses, closest-hit segment queries, and material friction/restitution. Its accumulator caps catch-up steps after stalls or hidden tabs. Call it once per simulation frame; do not also advance its backend.

Vehicles attach a configurable array of suspension rays to a dynamic chassis. Each wheel supplies local position, radius, steering/driven/braking flags, suspension travel, stiffness, damping and grip. Chassis mass, drive force, steering angle and brakes remain tuning data. Four wheels are the playground's authored choice; the engine supports other arrangements. Both human and AI controllers command the same character response.

Raycast tires are a first vehicle foundation: they have no solid rolling wheel collider, and the underlying discrete solver can tunnel through thin geometry at high speeds. Use sufficiently thick barriers and moderate speeds. Suspension stiffness/damping and tire grip use the solver's tuning conventions; `brakeForce` is a per-wheel maximum impulse per fixed step. These are not a full drivetrain, tire or suspension engineering simulation.

## Authoring workflow
Run `npm run dev:editor`, choose **VEHICLE PLAYGROUND**, and open `assets/playground.joylevel`. Use the level outliner and viewport to select/place entities, edit transforms and component records, and undo/redo changes. **Save File** writes the same file used by VS Code, with the workspace's revision checks and recovery copies. **New Level** creates a validated `.joylevel` without overwriting an existing file.

The game preview loads saved authored state. Saved level changes are staged for an explicit reset/regeneration so a physics session never silently jumps to a new layout. Editing a file and changing a live simulation are separate operations. Stop unloads the preview and its resources; Run creates a fresh session.

`npm run dev:vehicle-playground` runs the example directly. It exercises player/AI possession, configurable wheel layouts, collision bodies, ramps and live diagnostics. The project README describes controls and tuning.
