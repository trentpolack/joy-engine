import test from 'node:test';
import assert from 'node:assert/strict';
import * as objects from '../src/gameplay/object-document.ts';
import { LevelDocument, parseLevel, serializeLevel } from '../src/gameplay/level-document.ts';

const source = () => ({version: 1, name: 'Rain vessel', entity: {id: 'object', type: 'vessel', tags: ['wet'], components: {render: {asset: 'assets/vessel.form'}, gameplay: {water: 10, enabled: true, custom: {label: 'Rain'}}}}});

test('linked objects inherit source changes while local overrides and instance identity survive history and serialization', () => {
  const object = objects.parseObject(JSON.stringify(source()));
  const entity = objects.createObjectInstance(object, 'assets/vessel.joyobject', 'vessel-1');
  const doc = new LevelDocument({version: 1, name: 'Garden', entities: [entity]});
  doc.transact(level => {level.entities[0].overrides = {components: {gameplay: {water: 30}}};});
  const changed = source();
  changed.entity.components.gameplay.enabled = false;
  const resolved = objects.resolveObjectInstance(doc.snapshot().entities[0], objects.parseObject(JSON.stringify(changed)));
  assert.equal(resolved.id, 'vessel-1');
  assert.equal(resolved.type, 'vessel');
  assert.equal(resolved.components.gameplay.water, 30);
  assert.equal(resolved.components.gameplay.enabled, false);
  assert.equal(resolved.components.gameplay.custom.label, 'Rain');
  doc.undo();
  assert.equal(objects.resolveObjectInstance(doc.snapshot().entities[0], object).components.gameplay.water, 10);
  doc.redo();
  assert.deepEqual(parseLevel(serializeLevel(doc.snapshot())), doc.snapshot());
  resolved.components.gameplay.water = 99;
  assert.equal(object.entity.components.gameplay.water, 10);
});

test('object documents reject nested links, unsafe paths, and malformed overrides without losing custom values', () => {
  const value = source();
  value.entity.template = 'assets/recursive.joyobject';
  assert.throws(() => objects.parseObject(JSON.stringify(value)), /nested|template/i);
  const object = objects.parseObject(JSON.stringify(source()));
  assert.throws(() => objects.createObjectInstance(object, '../outside.joyobject', 'x'), /path|assets/i);
  const entity = objects.createObjectInstance(object, 'assets/vessel.joyobject', 'x');
  entity.overrides = {components: {gameplay: null}};
  assert.equal(objects.resolveObjectInstance(entity, object).components.gameplay, undefined);
  assert.deepEqual(objects.parseObject(objects.serializeObject(object)), object);
});

test('level resolution reads shared object sources once and leaves authored records untouched', async () => {
  const object = objects.parseObject(JSON.stringify(source()));
  const level = {version: 1, name: 'Garden', entities: ['a', 'b'].map(id => objects.createObjectInstance(object, 'assets/vessel.joyobject', id))};
  let reads = 0;
  const resolved = await objects.resolveLevelObjects(level, async path => {reads++; assert.equal(path, 'assets/vessel.joyobject'); return JSON.stringify(source());});
  assert.equal(reads, 1);
  assert.equal(resolved.entities[0].template, undefined);
  assert.equal(level.entities[0].template, 'assets/vessel.joyobject');
  resolved.entities[0].components.gameplay.water = 99;
  assert.equal(resolved.entities[1].components.gameplay.water, 10);
});
