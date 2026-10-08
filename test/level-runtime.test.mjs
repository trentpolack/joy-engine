// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import { LevelRuntime } from '../src/gameplay/level-runtime.ts';
import { EntityRegistry } from '../src/gameplay/entity-registry.ts';
import { Entity } from '../src/gameplay/entity.ts';
import { PhysicsWorld } from '../src/physics/physics-world.ts';
import { parseLevel, serializeLevel } from '../src/gameplay/level-document.ts';

const registry = () => new EntityRegistry().register('prop', definition => new Entity(definition));
const definition = (id, y, mass) => ({id, name: id, type: 'prop', tags: [], transform: {position: [0, y, 0], rotation: [0, 0, 0], scale: [2, 1, 2]}, components: {physics: {shape: 'box', halfExtents: [0.5, 0.5, 0.5], mass}, render: {shape: 'box', size: [1, 1, 1], color: [1, 1, 1]}, custom: {preserved: true}}});

test('runtime copies authoring data, scales collision dimensions and releases resources on repeated disposal', () => {
  const authored = {version: 1, name: 'Drop', entities: [definition('ground', -0.5, 0), definition('box', 4, 1)]};
  const before = serializeLevel(authored);
  const runtime = new LevelRuntime(authored, {registry: registry()});
  for(let index = 0; index < 180; index++) {
    runtime.step(1/60);
  }
  assert.ok(Math.abs(runtime.world.get('box').transform.position[1] - 0.5) < 0.12);
  assert.equal(serializeLevel(authored), before);
  assert.deepEqual(parseLevel(before).entities[1].components.custom, {preserved: true});
  const body = runtime.getBody('box');
  runtime.world.remove('box');
  assert.equal(body.disposed, true);
  runtime.destroy();
  runtime.destroy();
  assert.equal(runtime.world.entities.size, 0);
});

test('a malformed later component rolls back all earlier runtime resources in a borrowed physics world', () => {
  const physics = new PhysicsWorld();
  const linked = {...definition('linked', 0, 0), template: 'assets/prop.joyobject'};
  assert.throws(() => new LevelRuntime({version: 1, name: 'Unresolved', entities: [linked]}, {registry: registry(), physics}), /resolveLevelObjects/);
  assert.equal(physics.bodies.size, 0);
  const ground = definition('ground', -0.5, 0);
  const invalid = definition('invalid', 4, 1);
  invalid.components.physics.halfExtents = [1, -2, 1];
  assert.throws(() => new LevelRuntime({version: 1, name: 'Invalid', entities: [ground, invalid]}, {registry: registry(), physics}));
  assert.equal(physics.bodies.size, 0);
  physics.destroy();
});

test('uniform vehicle scaling preserves local wheel geometry and nonuniform scaling rolls back', () => {
  const kart = definition('kart', 4, 10);
  kart.transform.scale = [2, 2, 2];
  kart.components.vehicle = {wheels: [{position: [0.5, 0, 1], radius: 0.3, suspensionRestLength: 0.2, driven: true}]};
  const runtime = new LevelRuntime({version: 1, name: 'Scaled', entities: [kart]}, {registry: registry()});
  const wheel = runtime.getVehicle('kart').getWheelTransforms()[0];
  assert.equal(wheel.position[0], 1);
  assert.equal(wheel.position[2], 2);
  runtime.destroy();
  kart.transform.scale = [2, 1, 2];
  const physics = new PhysicsWorld();
  assert.throws(() => new LevelRuntime({version: 1, name: 'Invalid scale', entities: [kart]}, {registry: registry(), physics}), /uniform/);
  assert.equal(physics.bodies.size, 0);
  physics.destroy();
});

test('throwing project cleanup cannot retain the runtime solver', () => {
  class ThrowsOnDestroy extends Entity {
    onDestroy() { throw new Error('project cleanup failed'); }
  }
  const types = new EntityRegistry().register('prop', definition => new ThrowsOnDestroy(definition));
  const runtime = new LevelRuntime({version: 1, name: 'Cleanup', entities: [definition('box', 2, 1)]}, {registry: types});
  assert.throws(() => runtime.destroy(), /cleanup failed/);
  assert.equal(runtime.physics.disposed, true);
  assert.equal(runtime.physics.bodies.size, 0);
  runtime.destroy();
});

test('same-id entity replacement cannot reuse disposed runtime physics handles', () => {
  for(const inspectBeforeStep of [false, true]) {
    const kart = definition('kart', 4, 10);
    kart.transform.scale = [1, 1, 1];
    kart.components.vehicle = {wheels: [{position: [0, 0, 0], driven: true}]};
    const runtime = new LevelRuntime({version: 1, name: 'Replacement', entities: [kart]}, {registry: registry()});
    const body = runtime.getBody('kart');
    const vehicle = runtime.getVehicle('kart');
    runtime.world.remove('kart');
    kart.transform.position = [3, 9, 2];
    const replacement = runtime.world.spawn(kart);
    assert.equal(body.disposed, true);
    assert.equal(vehicle.disposed, true);
    if(inspectBeforeStep) {
      assert.equal(runtime.getVehicle('kart'), null);
      assert.equal(runtime.getBody('kart'), null);
    }
    assert.doesNotThrow(() => runtime.step(0));
    assert.deepEqual(replacement.transform.position, [3, 9, 2]);
    assert.equal(runtime.getBody('kart'), null);
    assert.equal(runtime.getVehicle('kart'), null);
    assert.equal(runtime.bodies.size, 0);
    assert.equal(runtime.vehicles.size, 0);
    assert.equal(runtime.physics.bodies.size, 0);
    assert.equal(runtime.physics.vehicles.size, 0);
    runtime.destroy();
  }
});
