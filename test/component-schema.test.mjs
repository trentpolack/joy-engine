// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import test from 'node:test';
import assert from 'node:assert/strict';
import { ComponentSchemaCatalog, LevelScene, LevelRuntime, EntityRegistry } from '../src/index.ts';
import { parseLevel, serializeLevel } from '../src/gameplay/level-document.ts';

const catalog = new ComponentSchemaCatalog();

/**
 * Build an authored entity at the origin with the given components.
 * @param {string} id
 * @param {Record<string, import('../src/index.ts').JsonRecord>} components
 */
function entityWith(id, components) {
  return {id, name: id, type: 'entity', tags: [], transform: {position: [0, 1, 0], rotation: [0, 0, 0], scale: [1, 1, 1]}, components};
}

test('built-in presets author owned data that round trips and runs in the engine runtimes', () => {
  const components = {};
  for(const schema of catalog.list()) {
    for(const preset of schema.presets) {
      const created = catalog.createComponent(preset.id);
      assert.equal(created.type, schema.type);
      created.data.mutated = true;
      assert.equal(Object.hasOwn(catalog.createComponent(preset.id).data, 'mutated'), false, `${preset.id} must return a copy`);
      delete created.data.mutated;
      components[preset.id] = created;
    }
  }

  // Only unassigned required assets are reported; every other preset value satisfies its schema.
  const issues = Object.values(components).flatMap(({type, data}) => catalog.validate(type, data));
  assert.deepEqual(issues.map(issue => `${issue.component}.${issue.field}`), ['effect.asset']);

  // A light, primitive and collider vehicle from presets survive a document round trip and spawn.
  const kart = entityWith('kart', {
    render: catalog.createComponent('render.primitive').data,
    physics: {...catalog.createComponent('physics.box').data, mass: 120},
    vehicle: catalog.createComponent('vehicle.four-wheel').data
  });
  const lamp = entityWith('lamp', {light: catalog.createComponent('light.point').data});
  const level = parseLevel(serializeLevel({version: 1, name: 'Presets', entities: [kart, lamp]}));
  assert.deepEqual(level.entities.flatMap(entity => catalog.validateEntity(entity)), []);

  const scene = new LevelScene();
  scene.sync(level.entities);
  assert.equal(scene.scene.instances.size, 1);
  assert.equal(scene.lights.length, 1);
  scene.destroy();

  const runtime = new LevelRuntime(level, {registry: new EntityRegistry()});
  assert.ok(runtime.getVehicle('kart'));
  runtime.destroy();
});

test('validation reports authored mistakes by field path without rejecting custom data', () => {
  const light = catalog.validate('light', {color: [1, -0.5, 1], intensity: 'bright', range: 0, castsShadow: true, flicker: 3});
  assert.deepEqual(light.map(issue => issue.field), ['color', 'intensity', 'range']);
  assert.match(light[2].message, /greater than 0/);

  // Conditional fields apply only to the matching shape; required companions are entity-level.
  assert.deepEqual(catalog.validate('physics', {shape: 'box', radius: -1}), []);
  assert.deepEqual(catalog.validate('physics', {shape: 'sphere', radius: -1}).map(issue => issue.field), ['radius']);
  assert.deepEqual(catalog.validate('physics', {mass: 1}).map(issue => issue.field), ['shape']);

  const vehicle = {wheels: [{position: [0, 0, 1]}, {position: [0, 0], radius: 0}, 7]};
  assert.deepEqual(catalog.validate('vehicle', vehicle).map(issue => issue.field), ['wheels.1.position', 'wheels.1.radius', 'wheels.2']);
  const loose = catalog.validateEntity(entityWith('loose', {vehicle: {wheels: []}, gameplay: {anything: [1, 'two']}}));
  assert.deepEqual(loose.map(issue => [issue.component, issue.field]), [['vehicle', null]]);

  assert.deepEqual(catalog.validate('effect', {asset: 'assets/fire.form'}).map(issue => issue.field), ['asset']);
  assert.deepEqual(catalog.validate('gameplay', {anything: true}), []);
});

test('project schemas register beside built-ins and resolve nested fields for tools', () => {
  const project = new ComponentSchemaCatalog();
  project.register({
    type: 'spawner', label: 'Spawner', description: 'Spawns waves.', category: 'Gameplay',
    fields: [{name: 'interval', kind: 'number', unit: 's', min: 0, minExclusive: true, default: 2}],
    presets: [{id: 'spawner.default', label: 'Spawner', data: {interval: 2}}]
  });
  assert.equal(project.get('spawner')?.fields[0].unit, 's');
  assert.equal(new ComponentSchemaCatalog().has('spawner'), false, 'catalogs are independent');
  assert.equal(Object.isFrozen(project.get('spawner')?.fields), true);

  assert.equal(project.field('vehicle', ['wheels', '3', 'radius'])?.unit, 'm');
  assert.equal(project.field('vehicle', ['wheels'])?.kind, 'list');
  assert.equal(project.field('vehicle', ['wheels', '3']), null, 'a list item is a record, not a field');
  assert.equal(project.field('render', ['parameters', 'height']), null, 'free-form records stay custom');

  const valid = {type: 'x', label: 'X', description: '', category: 'Gameplay', fields: [], presets: [{id: 'x.default', label: 'X', data: {}}]};
  assert.throws(() => project.register({...valid, type: 'spawner'}), /already registered/);
  assert.throws(() => project.register({...valid, presets: [{id: 'render.primitive', label: 'Copy', data: {}}]}), /preset render.primitive/);
  assert.throws(() => project.register({...valid, fields: [{name: 'size', kind: 'vector3', when: {field: 'missing', equals: ['a']}}]}), /sibling choice/);
  assert.throws(() => project.register({...valid, fields: [{name: 'mode', kind: 'choice'}]}), /requires choices/);
  assert.equal(project.has('x'), false, 'rejected schemas leave the catalog unchanged');
});
