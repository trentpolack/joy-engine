import test from 'node:test';
import assert from 'node:assert/strict';
import {
  Entity, Character, PlayerCharacter, Controller, PlayerController, AIController,
  EntityRegistry, EntityWorld, EntityComponent, ComponentRegistry, EntitySystem
} from '../src/gameplay/index.ts';

test('exclusive possession transfers clear both characters and controllers, including destruction', () => {
  const first = new PlayerCharacter({id: 'first'});
  const second = new Character({id: 'second'});
  const player = new PlayerController({sampleInput: () => ({throttle: 1, brake: false})});
  const ai = new AIController({sampleIntent: () => ({throttle: 0.5})});
  player.possess(first);
  player.update(0.1);
  assert.deepEqual(first.controlIntent, {throttle: 1, brake: false});
  ai.possess(first);
  assert.equal(player.character, null);
  assert.equal(first.controller, ai);
  assert.deepEqual(first.controlIntent, {});
  ai.update(0.1);
  ai.possess(second);
  assert.equal(first.controller, null);
  assert.deepEqual(first.controlIntent, {});
  ai.update(0.1);
  second.destroy();
  assert.equal(ai.character, null);
  assert.deepEqual(second.controlIntent, {});
  assert.throws(() => player.possess(second), /destroyed/);
  player.possess(first);
  player.update(0.1);
  player.destroy();
  player.destroy();
  assert.equal(first.controller, null);
  assert.deepEqual(first.controlIntent, {});
});

test('input samples replace stale keys, copy borrowed data, and fail without leaving active input', () => {
  let sample = {throttle: 1, brake: true};
  const character = new Character({id: 'car'});
  const controller = new PlayerController({sampleInput: () => sample});
  controller.possess(character);
  controller.update(0.016);
  sample.throttle = 0;
  assert.equal(character.controlIntent.throttle, 1);
  sample = {throttle: 0.5};
  controller.update(0.016);
  assert.deepEqual(character.controlIntent, {throttle: 0.5});
  sample = {throttle: NaN};
  assert.throws(() => controller.update(0.016), /finite/);
  assert.deepEqual(character.controlIntent, {});
  sample = {throttle: 1};
  controller.update(0.016);
  controller.sampleInput = () => { throw new Error('input disconnected'); };
  assert.throws(() => controller.update(0.016), /disconnected/);
  assert.deepEqual(character.controlIntent, {});
  assert.throws(() => controller.update(-1), /seconds/);
  controller.destroy();
});

test('world factories preserve custom components while independent worlds own copied runtime data', () => {
  class Kart extends Character {
    update(dt) {
      this.transform.position[0]+= this.controlIntent.throttle*dt;
    }
  }
  const registry = new EntityRegistry().register('kart', (definition) => new Kart(definition));
  const first = new EntityWorld({registry});
  const second = new EntityWorld({registry});
  const definition = {
    id: 'kart', type: 'kart', tags: ['vehicle'],
    transform: {position: [2, 0, 0]},
    components: {vehicle: {wheels: [{driven: true}], extension: {value: 42}}}
  };
  const a = first.spawn(definition);
  const b = second.spawn(definition);
  a.components.vehicle.wheels[0].driven = false;
  a.receiveControl({throttle: 3});
  first.update(0.5);
  assert.equal(a.transform.position[0], 3.5);
  assert.equal(b.transform.position[0], 2);
  assert.equal(definition.components.vehicle.wheels[0].driven, true);
  assert.equal(b.components.vehicle.wheels[0].driven, true);
  assert.throws(() => first.spawn(definition), /already/);
  assert.throws(() => registry.register('kart', () => new Entity({id: 'x'})), /already/);
  first.destroy();
  assert.equal(a.destroyed, true);
  assert.equal(b.destroyed, false);
  assert.equal(first.get('kart'), null);
  second.destroy();
});

test('failed spawn rolls back owned resources and possession without publishing a partial entity', () => {
  const controller = new Controller();
  let released = 0;
  let ownedReleased = 0;
  let failed;
  class Broken extends Character {
    onSpawn(world) {
      assert.equal(world.get(this.id), this);
      this.own({destroy: () => { ownedReleased+= 1; }});
      controller.possess(this);
      this.receiveControl({throttle: 1});
      throw new Error('resource setup failed');
    }
    onDestroy() {
      released+= 1;
    }
  }
  const registry = new EntityRegistry().register('broken', (definition) => {
    failed = new Broken(definition);
    return failed;
  });
  const world = new EntityWorld({registry});
  assert.throws(() => world.spawn({id: 'broken', type: 'broken'}), /resource setup failed/);
  assert.equal(world.get('broken'), null);
  assert.equal(controller.character, null);
  assert.equal(failed.world, null);
  assert.equal(failed.destroyed, true);
  assert.deepEqual(failed.controlIntent, {});
  assert.equal(released, 1);
  assert.equal(ownedReleased, 1);
  world.spawn({id: 'healthy'});
  world.destroy();
  assert.equal(released, 1);
});

test('world updates a stable spawn order and skips removals until the next frame for new entities', () => {
  const events = [];
  class Actor extends Entity {
    update() {
      events.push(this.id);
      if(this.id === 'first') {
        this.world.remove('second');
        if(!this.world.get('third')) {
          this.world.spawn({id: 'third', type: 'actor'});
        }
      }
    }
  }
  const registry = new EntityRegistry().register('actor', (definition) => new Actor(definition));
  const world = new EntityWorld({registry});
  world.spawn({id: 'first', type: 'actor'});
  world.spawn({id: 'second', type: 'actor'});
  world.update(0.1);
  assert.deepEqual(events, ['first']);
  world.update(0.1);
  assert.deepEqual(events, ['first', 'first', 'third']);
  world.destroy();
});

test('cleanup errors do not strand other entities and disposed worlds reject new lifetimes', () => {
  const released = [];
  class Resource extends Character {
    onDestroy() {
      released.push(this.id);
      if(this.id === 'bad') {
        throw new Error('release failed');
      }
    }
  }
  const registry = new EntityRegistry().register('resource', (definition) => new Resource(definition));
  const world = new EntityWorld({registry});
  const bad = world.spawn({id: 'bad', type: 'resource'});
  const good = world.spawn({id: 'good', type: 'resource'});
  const controller = new Controller();
  controller.possess(bad);
  assert.throws(() => world.destroy(), /cleanup/);
  assert.deepEqual(released, ['bad', 'good']);
  assert.equal(world.entities.size, 0);
  assert.equal(bad.world, null);
  assert.equal(good.world, null);
  assert.equal(controller.character, null);
  world.destroy();
  assert.throws(() => world.spawn({id: 'late'}), /destroyed/);
});

test('definition and factory validation reject non-JSON data and inconsistent ownership', () => {
  assert.throws(() => new Entity({id: 'bad', components: {data: {value: undefined}}}), /JSON/);
  assert.throws(() => new Entity({id: 'bad', transform: {position: [0, Infinity, 0]}}), /finite/);
  const cyclic = {};
  cyclic.self = cyclic;
  assert.throws(() => new Entity({id: 'bad', components: {data: cyclic}}), /cyclic/);
  const registry = new EntityRegistry().register('wrong', () => new Entity({id: 'wrong-id'}));
  const world = new EntityWorld({registry});
  assert.throws(() => world.spawn({id: 'requested', type: 'wrong'}), /id/);
  assert.equal(world.entities.size, 0);
  assert.throws(() => world.spawn({id: 'unknown', type: 'missing'}), /Unknown/);
  registry.register('rewritten', (definition) => {
    definition.id = 'rewritten';
    return new Entity(definition);
  });
  assert.throws(() => world.spawn({id: 'original', type: 'rewritten'}), /id/);
  assert.equal(world.entities.size, 0);
  world.destroy();
});


test('entity-owned resources release in reverse order even if subclass or resource cleanup fails', () => {
  const released = [];
  class Owner extends Entity {
    onDestroy() {
      released.push('hook');
      throw new Error('hook failed');
    }
  }
  const entity = new Owner({id: 'owner'});
  const dependency = {destroy: () => released.push('dependency')};
  assert.equal(entity.own(dependency), dependency);
  entity.own(dependency);
  entity.own({destroy: () => {
    released.push('dependent');
    throw new Error('dependent failed');
  }});
  assert.throws(() => entity.destroy(), (error) => {
    assert.equal(error.errors.length, 2);
    return true;
  });
  assert.deepEqual(released, ['hook', 'dependent', 'dependency']);
  entity.destroy();
  assert.equal(released.length, 3);
  assert.throws(() => entity.own(dependency), /destroyed/);
});

test('possession commits both backreferences before reset hooks can reenter', () => {
  const controller = new Controller();
  const first = new Character({id: 'first'});
  const second = new Character({id: 'second'});
  controller.possess(first);
  first.resetControl = () => {
    first.controlIntent = {};
    controller.possess(first);
  };
  controller.possess(second);
  assert.equal(controller.character, first);
  assert.equal(first.controller, controller);
  assert.equal(second.controller, null);
  first.resetControl = Character.prototype.resetControl;
  controller.destroy();
});

test('destruction rejects reset-hook reacquisition and releases resources after hook failure', () => {
  const registry = new EntityRegistry();
  const world = new EntityWorld({registry});
  const character = world.spawn({id: 'character', type: 'character'});
  const controller = new Controller();
  let released = 0;
  character.own({destroy: () => { released+= 1; }});
  controller.possess(character);
  character.controlIntent = {throttle: 1};
  character.resetControl = () => controller.possess(character);
  assert.throws(() => character.destroy(), /cleanup/);
  assert.equal(character.destroyed, true);
  assert.equal(world.get('character'), null);
  assert.equal(character.controller, null);
  assert.equal(controller.character, null);
  assert.deepEqual(character.controlIntent, {});
  assert.equal(released, 1);
  character.destroy();
  assert.equal(released, 1);
  world.destroy();
});

test('reset hook errors preserve exclusive ownership and clear both stale intents', () => {
  const first = new Character({id: 'first'});
  const second = new Character({id: 'second'});
  const controller = new Controller();
  const displaced = new Controller();
  controller.possess(first);
  displaced.possess(second);
  first.controlIntent = {throttle: 1};
  second.controlIntent = {throttle: 0.5};
  first.resetControl = () => { throw new Error('reset failed'); };
  assert.throws(() => controller.possess(second), /reset/);
  assert.equal(first.controller, null);
  assert.equal(second.controller, controller);
  assert.equal(controller.character, second);
  assert.equal(displaced.character, null);
  assert.deepEqual(first.controlIntent, {});
  assert.deepEqual(second.controlIntent, {});
  controller.destroy();
});

test('sparse transform coordinates cannot enter runtime or authored definitions', () => {
  for(const field of ['position', 'rotation', 'scale']) {
    assert.throws(() => new Entity({id: 'sparse', transform: {[field]: [0, , 0]}}), /finite/);
  }
});

test('registered components compose data-driven behavior without replacing the base entity', () => {
  const events = [];
  class HealthComponent extends EntityComponent {
    constructor(data) {
      super('health', data);
    }
    onAttach(entity) {
      events.push(`attach:${entity.id}:${this.data.current}`);
      this.own({destroy: () => events.push('resource')});
    }
    onDetach() {
      events.push('detach');
    }
  }

  const componentRegistry = new ComponentRegistry()
    .register('health', data => new HealthComponent(data));
  const world = new EntityWorld({componentRegistry});
  const entity = world.spawn({id: 'villager', components: {health: {current: 10}, cosmetic: {hat: 'red'}}});
  const health = entity.getComponent('health');

  assert.equal(health.data, entity.components.health);
  assert.equal(health.world, world);
  assert.equal(entity.getComponent('cosmetic'), null);
  assert.deepEqual([...entity.runtimeComponents.keys()], ['health']);
  entity.components.health.current = 8;
  assert.equal(health.data.current, 8);

  world.destroy();
  assert.equal(health.entity, null);
  assert.equal(health.destroyed, true);
  assert.deepEqual(events, ['attach:villager:10', 'detach', 'resource']);
});

test('systems query component composition in stable order and defer frame spawns', () => {
  const updates = [];
  class Marker extends EntityComponent {
    constructor(data) {
      super('marker', data);
    }
  }
  class MarkerSystem extends EntitySystem {
    constructor() {
      super({requiredComponents: ['marker']});
    }
    update(seconds, entities) {
      updates.push([seconds, ...entities.map(entity => entity.id)]);
      if(!this.world.get('late')) {
        this.world.spawn({id: 'late', components: {marker: {}}});
      }
    }
  }

  const componentRegistry = new ComponentRegistry().register('marker', data => new Marker(data));
  const world = new EntityWorld({componentRegistry});
  world.spawn({id: 'first', components: {marker: {}}});
  world.spawn({id: 'ignored'});
  const system = world.addSystem(new MarkerSystem());

  assert.deepEqual(world.query(['marker']).map(entity => entity.id), ['first']);
  world.update(0.25);
  world.update(0.5);
  assert.deepEqual(updates, [[0.25, 'first'], [0.5, 'first', 'late']]);

  system.destroy();
  assert.equal(system.world, null);
  assert.equal(system.destroyed, true);
  assert.equal(world.systems.length, 0);
  world.destroy();
});

test('failed component attachment rolls back the complete entity spawn', () => {
  const released = [];
  class BrokenComponent extends EntityComponent {
    constructor(data) {
      super('broken', data);
    }
    onAttach() {
      this.own({destroy: () => released.push('resource')});
      throw new Error('component setup failed');
    }
    onDetach() {
      released.push('component');
    }
  }

  const componentRegistry = new ComponentRegistry().register('broken', data => new BrokenComponent(data));
  const world = new EntityWorld({componentRegistry});
  assert.throws(() => world.spawn({id: 'broken', components: {broken: {}}}), /component setup failed/);
  assert.equal(world.get('broken'), null);
  assert.deepEqual(released, ['component', 'resource']);
  world.destroy();
});
