// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, symlink, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { LevelDocument, parseLevel, serializeLevel } from 'joy-engine/levels';
import { LevelScene } from 'joy-engine';
import { ProjectFiles } from '../server/project-files.mjs';

const entity = {id: 'box', name: 'Box', type: 'Entity', tags: [], transform: {position: [0, 1, 0], rotation: [0, 0, 0], scale: [1, 1, 1]}, components: {custom: {nested: [1, {label: 'retain me'}]}}};
const level = {version: 1, name: 'Track', entities: [entity]};

test('levels round trip unknown components and reject invalid authored state', () => {
  assert.deepEqual(parseLevel(serializeLevel(level)), level);
  assert.throws(() => parseLevel(JSON.stringify({...level, version: 2})), /version/i);
  assert.throws(() => parseLevel(JSON.stringify({...level, entities: [entity, entity]})), /duplicate/i);
  assert.throws(() => serializeLevel({...level, entities: [{...entity, transform: {...entity.transform, position: [NaN, 0, 0]}}]}), /finite/i);
});

test('transactions isolate snapshots, roll back invalid changes and bound undo history', () => {
  const document = new LevelDocument(level);
  document.select('box');
  const snapshot = document.snapshot();
  snapshot.entities[0].name = 'mutated copy';
  assert.equal(document.snapshot().entities[0].name, 'Box');
  assert.throws(() => document.transact(draft => {
    draft.entities.push(draft.entities[0]);
  }), /duplicate/i);
  assert.equal(document.canUndo, false);
  document.transact(draft => {
    draft.entities[0].name = 'Edited';
  });
  document.undo();
  assert.equal(document.snapshot().entities[0].name, 'Box');
  document.redo();
  assert.equal(document.snapshot().entities[0].name, 'Edited');
  for(let index = 0; index < 100; index++) {
    document.transact(draft => {
      draft.name = `Edit ${index}`;
    });
  }
  let undos = 0;
  while(document.undo()) {
    undos++;
  }
  assert.equal(undos, 80);
  document.transact(draft => {
    draft.entities = [];
  });
  assert.equal(document.selectedId, null);
  assert.equal(document.canRedo, false);
});

test('new levels use exclusive creation, reject traversal and links, and retain revision conflicts', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'joy-level-'));
  try {
    await mkdir(path.join(root, 'projects/demo/assets'), {recursive: true});
    await writeFile(path.join(root, 'projects/demo/package.json'), '{}');
    const files = new ProjectFiles(root);
    const text = serializeLevel(level);
    const outcomes = await Promise.allSettled([files.create('demo', 'assets/track.joylevel', text), files.create('demo', 'assets/track.joylevel', text)]);
    assert.equal(outcomes.filter(result => result.status === 'fulfilled').length, 1);
    assert.equal(await readFile(path.join(root, 'projects/demo/assets/track.joylevel'), 'utf8'), text);
    const original = await files.read('demo', 'assets/track.joylevel');
    await files.save('demo', 'assets/track.joylevel', text + '\n', original.revision);
    await assert.rejects(files.save('demo', 'assets/track.joylevel', text, original.revision), /changed/i);
    await assert.rejects(files.create('demo', 'assets/../evil.joylevel', text));
    await assert.rejects(files.create('demo', 'assets/code.js', text));
    await assert.rejects(files.create('demo', 'assets/bad.joylevel', '{broken'));
    await mkdir(path.join(root, 'outside'));
    await symlink(path.join(root, 'outside'), path.join(root, 'projects/demo/assets/link'));
    await assert.rejects(files.create('demo', 'assets/link/escape.joylevel', text));
    await symlink(path.join(root, 'outside/missing'), path.join(root, 'projects/demo/assets/symlink.joylevel'));
    await assert.rejects(files.create('demo', 'assets/symlink.joylevel', text));
  } finally {
    await rm(root, {recursive: true, force: true});
  }
});

test('level presentation retains instances and combines full render size with transform scale', () => {
  const scene = new LevelScene();
  const definition = structuredClone(entity);
  definition.components.render = {shape: 'box', size: [2, 3, 4], color: [0.2, 0.4, 0.6]};
  definition.transform.scale = [3, 2, 1];
  scene.sync([definition]);
  const instance = [...scene.scene.instances][0];
  assert.deepEqual(instance.scale, [6, 6, 4]);
  definition.transform.position[0] = 9;
  scene.sync([definition]);
  assert.equal([...scene.scene.instances][0], instance);
  assert.equal(instance.position[0], 9);
  scene.sync([]);
  assert.equal(scene.scene.instances.size, 0);
  scene.destroy();
  scene.destroy();
});
