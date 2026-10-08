# Gameplay API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## Entity
Import from `joy-engine`. [Source](../../src/gameplay/entity.ts)

Instance-owned runtime data. Subclasses acquire resources in onSpawn and release them in onDestroy.

### constructor
```ts
constructor(definition: EntityOptions)
```
- **param** definition

### onSpawn
```ts
onSpawn(_world: EntityWorld)
```
Acquire subclass resources after registration. Throwing triggers onDestroy rollback. Factories should construct data only: a constructor that throws must clean up its own resources.

- **param** _world Borrowed owning world.

### update
```ts
update(_elapsedSeconds: number)
```
Subclass simulation hook; the base entity has no update behavior.

- **param** _elapsedSeconds Nonnegative simulation seconds, supplied by the world.

### addComponent
```ts
addComponent<T extends EntityComponent>(component: T): T
```
Attach owned runtime behavior. Authored data remains available through components.

- **param** component Fresh unowned component.

### getComponent
```ts
getComponent<T extends EntityComponent = EntityComponent>(type: string): T | null
```
Return borrowed runtime behavior for a component type, or null.


### removeComponent
```ts
removeComponent(type: string): boolean
```
Detach and destroy owned runtime behavior.


### own
```ts
own<T extends {
    destroy: () => void;
}>(resource: T): T
```
Transfer resource cleanup to this entity; returns the same resource for convenient setup. Acquire dependencies before dependents so reverse disposal preserves their lifetime order.

- **param** resource

### destroy
```ts
destroy()
```
Remove this instance from its world and release resources exactly once, even if cleanup throws.


### onDestroy
```ts
onDestroy()
```
Release subclass resources. Called even after partially completed onSpawn.



## EntityComponent
Import from `joy-engine`. [Source](../../src/gameplay/component.ts)

Runtime behavior attached to one entity. Authored component data remains a plain JSON record.

### constructor
```ts
constructor(type: string, data: TData)
```
Construct an unowned component. EntityWorld attaches registered components during spawn.

- **param** type Stable component type used by queries and registries.
- **param** data Entity-owned authored data; factories may retain and mutate this record.

### world
```ts
get world(): EntityWorld | null
```
Borrow the current world, or null before attachment and after removal.


### onAttach
```ts
onAttach(_entity: Entity)
```
Acquire resources after attachment. Throwing rolls the component back and releases them.


### onDetach
```ts
onDetach()
```
Release subclass state. The base path invokes this exactly once.


### own
```ts
own<T extends {destroy: () => void;}>(resource: T): T
```
Transfer resource cleanup to the component.

- **param** resource Owned disposable, released in reverse acquisition order.

### destroy
```ts
destroy()
```
Detach from the owning entity and release resources exactly once.



## ComponentRegistry
Import from `joy-engine`. [Source](../../src/gameplay/component-registry.ts)

Maps trusted component names to runtime behavior without evaluating authored data.

### constructor
```ts
constructor()
```

### register
```ts
register(type: string, factory: ComponentFactory): this
```
Register project behavior for an authored component type.

- **param** type Nonempty component type.
- **param** factory Borrowed factory; the returned component is owned by its entity.

### has
```ts
has(type: string): boolean
```
Return whether this registry can materialize runtime behavior for an authored type.


### create
```ts
create(type: string, data: JsonRecord, entity: Entity): EntityComponent
```
Construct an unowned runtime component over the entity's owned authored record.

- **param** type Registered component type.
- **param** data Entity-owned mutable component data.
- **param** entity Borrowed target entity.


## EntitySystem
Import from `joy-engine`. [Source](../../src/gameplay/entity-system.ts)

World-owned processor for entities selected by runtime component composition.

### constructor
```ts
constructor({requiredComponents = []}: EntitySystemOptions = {})
```
- **param** options Query constraints applied before every update.

### onAdd
```ts
onAdd(_world: EntityWorld)
```
Acquire system-wide resources after the world takes ownership.


### update
```ts
update(_elapsedSeconds: number, _entities: readonly Entity[])
```
Process a stable insertion-order snapshot. Elapsed time is finite nonnegative seconds.


### onRemove
```ts
onRemove()
```
Release system-wide resources exactly once.


### destroy
```ts
destroy()
```
Remove this system from its world, or finish an unowned system lifetime.



## Character
Import from `joy-engine`. [Source](../../src/gameplay/character.ts)

Gameplay owner that responds to controller intent; movement policy belongs to subclasses.

### constructor
```ts
constructor(definition: EntityOptions)
```
- **param** definition

### receiveControl
```ts
receiveControl(intent: ControlIntent)
```
Replace all actions, copying the caller's record. Invalid samples clear previous intent and throw.

- **param** intent Project-defined numeric or boolean actions.

### resetControl
```ts
resetControl()
```
Clear transient actions when possession changes, input fails, or simulation stops.


### destroy
```ts
destroy()
```
Release possession and all resources even if project reset or cleanup hooks throw.



## PlayerCharacter
Import from `joy-engine`. [Source](../../src/gameplay/character.ts)

Optional project extension point for player-specific character behavior. AI may also possess it.


## Controller
Import from `joy-engine`. [Source](../../src/gameplay/controller.ts)

Owns exclusive possession, but borrows the character and its world.

### constructor
```ts
constructor()
```
Start unpossessed; a later possess call borrows an existing character.


### possess
```ts
possess(character: Character | null)
```
Transfer exclusive possession, clearing both sides of the previous relationships and all stale intent. Passing null releases possession. Destroyed characters or controllers cannot be possessed. Reset hooks observe committed ownership; a reentrant possession supersedes this transition. Hook failures are reported after both intents are cleared and ownership is consistent.

- **param** character

### update
```ts
update(elapsedSeconds: number)
```
Sample before EntityWorld.update; controller scheduling belongs to the runtime composition root.

- **param** elapsedSeconds Nonnegative simulation seconds.

### destroy
```ts
destroy()
```
Release possession; the caller still owns borrowed input and the character. Repeated calls are harmless.



## PlayerController
Import from `joy-engine`. [Source](../../src/gameplay/player-controller.ts)

Samples borrowed keyboard/touch/gamepad adapters without owning their listeners.

### constructor
```ts
constructor({sampleInput}: PlayerControllerOptions)
```
- **param** options

### update
```ts
update(elapsedSeconds: number)
```
- **param** elapsedSeconds Nonnegative simulation seconds. Samples replace previous intent.


## AIController
Import from `joy-engine`. [Source](../../src/gameplay/player-controller.ts)

AI decisions use the same intent and possession contract as player input.

### constructor
```ts
constructor({sampleIntent}: AIControllerOptions)
```
- **param** options


## EntityRegistry
Import from `joy-engine`. [Source](../../src/gameplay/entity-registry.ts)

Trusted project factories interpret copied JSON; authored type names never evaluate code.

### constructor
```ts
constructor()
```
Register the built-in entity types for this independent factory collection.


### register
```ts
register(type: string, factory: EntityFactory): this
```
Register a trusted factory; duplicate names and invalid callbacks throw.

- **param** type Nonempty authored type name.
- **param** factory Borrowed creation callback.
- **returns** This registry for chained registration.

### create
```ts
create(definition: EntityOptions): Entity
```
Return a new unowned entity. The caller must destroy it or transfer it to a world. Factories must return a fresh instance with the requested id and acquire resources in onSpawn.

- **param** definition


## EntityWorld
Import from `joy-engine`. [Source](../../src/gameplay/entity-world.ts)

Owns entities and deterministic insertion-order updates, independently of rendering and controllers.

### constructor
```ts
constructor({registry = new EntityRegistry(), componentRegistry = new ComponentRegistry()}: EntityWorldOptions = {})
```
- **param** [options]

### spawn
```ts
spawn(definition: EntityOptions): Entity
```
Create and own an entity. Failed setup removes registration and invokes cleanup before throwing.

- **param** definition Copied authored input.
- **returns** Borrowed runtime instance.

### addSystem
```ts
addSystem<T extends EntitySystem>(system: T): T
```
Add an owned system at the end of deterministic update order.

- **param** system Fresh unowned system.

### removeSystem
```ts
removeSystem(system: EntitySystem): boolean
```
Remove and destroy an owned system.


### query
```ts
query(componentTypes: readonly string[]): Entity[]
```
Return a stable insertion-order snapshot matching every runtime component type.


### get
```ts
get(id: string): Entity | null
```
Look up a live registration without transferring ownership.

- **param** id
- **returns** Borrowed instance, or null.

### remove
```ts
remove(id: string): boolean
```
Remove ownership before cleanup, so reentrant callbacks never observe a half-destroyed entity. Cleanup failures propagate after the entity is detached.

- **param** id
- **returns** Whether a registered entity was removed.

### update
```ts
update(elapsedSeconds: number)
```
Update surviving entities in insertion order. New spawns wait for the next update.

- **param** elapsedSeconds Finite nonnegative simulation seconds.

### destroy
```ts
destroy()
```
Dispose every entity even if individual cleanup fails; report all errors after releasing ownership.



## copyEntityDefinition
Import from `joy-engine`. [Source](../../src/gameplay/entity-definition.ts)

```ts
function copyEntityDefinition(definition: EntityOptions): EntityDefinition
```
Validate and copy a definition, applying defaults without retaining authored references. Component values must be finite JSON records; custom keys survive unchanged.

- **param** definition


## LevelDocument
Import from `joy-engine`. [Source](../../src/gameplay/level-document.ts)

Owns authored state, selection and up to 80 undo transactions. Simulation must use snapshot().

### constructor
```ts
constructor(level: LevelData)
```
- **param** level

### snapshot
```ts
snapshot()
```
Return an owned copy suitable for rendering, simulation, or editing.


### canUndo
```ts
get canUndo()
```
Whether an earlier authored snapshot is available.


### canRedo
```ts
get canRedo()
```
Whether an undone authored snapshot is available.


### select
```ts
select(id: string | null)
```
Selection is UI state, not an authored edit.

- **param** id

### transact
```ts
transact(edit: (draft: LevelData) => void): boolean
```
Apply one atomic edit to an isolated draft. Invalid edits throw and retain prior state.

- **param** edit
- **returns** Whether authored data changed.

### undo
```ts
undo()
```
Restore one authored transaction; invalidated selections are cleared.


### redo
```ts
redo()
```
Reapply one undone transaction; invalidated selections are cleared.



## parseLevel
Import from `joy-engine`. [Source](../../src/gameplay/level-document.ts)

```ts
function parseLevel(text: string): LevelData
```
Parse a versioned authored level. Returns an owned copy; never evaluates component data.

- **param** text


## serializeLevel
Import from `joy-engine`. [Source](../../src/gameplay/level-document.ts)

```ts
function serializeLevel(level: LevelData): string
```
Validate and serialize a level without mutating its caller-owned data.

- **param** level


## LevelScene
Import from `joy-engine`. [Source](../../src/gameplay/level-scene.ts)

Shared retained placeholder geometry for authored levels and runtime previews. Owns its scene and instances. Transform scale multiplies render.size (full dimensions). Components with no render record are invisible; unknown records remain uninterpreted.

### constructor
```ts
constructor(resolveMesh: (entity: EntityDefinition) => MeshAsset | undefined = () => undefined)
```
- **param** [resolveMesh]

### sync
```ts
sync(definitions: readonly EntityDefinition[], selectedId: string | null = null)
```
Sync borrowed world-space definitions; copies presentation arrays, never mutates data.

- **param** definitions
- **param** [selectedId]

### destroy
```ts
destroy()
```
Release retained scene instances and their owned scene resources.



## LevelRuntime
Import from `joy-engine`. [Source](../../src/gameplay/level-runtime.ts)

Instantiates a copied level. Owns entities and their physics handles; a supplied physics world is borrowed. `physics` components use local collider dimensions; entity scale is applied once at construction. `vehicle` components use the PhysicsWorld vehicle contract, in chassis-local meters. Physics handles are attached during construction; later EntityWorld spawns require their own setup.

### constructor
```ts
constructor(level: LevelData, {registry, physics}: LevelRuntimeOptions)
```
- **param** level
- **param** options

### step
```ts
step(seconds: number)
```
Update character responses, advance bounded physics in seconds, and copy world transforms to entities. Controllers should be updated before this call. Authored definitions remain unchanged.

- **param** seconds

### getBody
```ts
getBody(id: string)
```
Borrow a live body.

- **param** id

### getVehicle
```ts
getVehicle(id: string)
```
Borrow a live raycast vehicle.

- **param** id

### destroy
```ts
destroy()
```
Release entities before the owned solver; repeated disposal is harmless.



## assetPath
Import from `joy-engine`. [Source](../../src/gameplay/asset-path.ts)

```ts
function assetPath(value: unknown, extensions: readonly string[] = []): string
```
Validate a project-relative authored asset reference without performing I/O.

- **param** value
- **param** [extensions]


## parseObject
Import from `joy-engine`. [Source](../../src/gameplay/object-document.ts)

```ts
function parseObject(text: string): ObjectData
```
Parse a reusable single-object definition; linked definitions cannot nest.

- **param** text


## serializeObject
Import from `joy-engine`. [Source](../../src/gameplay/object-document.ts)

```ts
function serializeObject(object: ObjectData)
```
Serialize a validated owned object without retaining caller references.

- **param** object


## createObjectInstance
Import from `joy-engine`. [Source](../../src/gameplay/object-document.ts)

```ts
function createObjectInstance(object: ObjectData, path: string, id: string)
```
Create an independently placed instance linked to one reusable definition.

- **param** object
- **param** path
- **param** id


## resolveObjectInstance
Import from `joy-engine`. [Source](../../src/gameplay/object-document.ts)

```ts
function resolveObjectInstance(entity: EntityDefinition, object: ObjectData)
```
Resolve explicit local overrides over current source data; identity and placement stay local. Null component overrides remove that component. Arrays replace as a unit.

- **param** entity
- **param** object


## resolveLevelObjects
Import from `joy-engine`. [Source](../../src/gameplay/object-document.ts)

```ts
async function resolveLevelObjects(level: LevelData, readText: (path: string) => Promise<string>): Promise<LevelData>
```
Resolve all linked objects with one source read per path; input data remains authored.

- **param** level
- **param** readText


## AssetScene
Import from `joy-engine`. [Source](../../src/gameplay/asset-scene.ts)

Own authored asset presentation, compiled caches and per-instance effect simulation. readText is borrowed and must enforce its project boundary. sync stages all resources before replacing the displayed result; update changes only live transforms.

### constructor
```ts
constructor({readText}: AssetSceneOptions)
```
- **param** options

### sync
```ts
async sync(entities: readonly EntityDefinition[], selectedId: string | null = null)
```
Resolve and prepare a replacement atomically. Returns false for a superseded load.

- **param** entities
- **param** [selectedId]

### cancelPending
```ts
cancelPending()
```
Invalidate pending loads while retaining the current scene. Call when a newer owner transaction starts before its own asset reads are ready.


### update
```ts
update(entities: readonly EntityDefinition[], selectedId: string | null = null)
```
Copy current transforms without reading assets or resetting effect time.

- **param** entities
- **param** [selectedId]

### step
```ts
step(seconds: number)
```
Advance effects in seconds, clamping background stalls and isolating script failures.

- **param** seconds

### particles
```ts
particles(camera: ParticleCameraBasis): number[]
```
Build camera-facing colored effects attached to each entity's transform.

- **param** camera

### destroy
```ts
destroy()
```
Dispose every effect and invalidate in-flight reads before releasing scene references.
