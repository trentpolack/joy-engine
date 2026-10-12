// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import {ChunkStructure, createChunkMeshData, createRemovalMutationBatch} from '../src/index.ts';

function wall() {
  const chunks = [];
  for(let row = 0; row < 3; row++) {
    for(let column = 0; column < 5; column++) {
      chunks.push({id:`wall:${column}:${row}`,center:[column - 2,row + 0.5,0],size:[1,1,0.5],material:'masonry',hitPoints:2});
    }
  }
  return chunks;
}

test('chunk damage commits geometry and traversal queries at one boundary', () => {
  const structure = new ChunkStructure(wall());
  const initialHealth = structure.damageState();
  assert.equal(initialHealth.find(chunk => chunk.chunkId === 'wall:2:0').remainingHitPoints, 2);
  const playerHalfExtents = [0.35,0.85,0.35];
  assert.equal(structure.canOccupy([0,0.85,0],playerHalfExtents),false);

  structure.queueDamage({center:[0,1,0],radius:1.1,damage:1});
  assert.deepEqual(structure.damageState(), initialHealth, 'Queued impacts cannot change committed diagnostics');
  assert.equal(structure.commit().removed.length,0);
  const damagedHealth = structure.damageState();
  assert.equal(damagedHealth.find(chunk => chunk.chunkId === 'wall:2:0').remainingHitPoints, 1);
  damagedHealth[0].remainingHitPoints = -100;
  assert.equal(structure.damageState()[0].remainingHitPoints, 2, 'Diagnostics must not expose mutable simulation state');
  structure.queueDamage({center:[0,1,0],radius:1.1,damage:1});
  const change = structure.commit();
  assert.deepEqual(structure.damageState().find(chunk => chunk.chunkId === 'wall:2:0'), {
    chunkId: 'wall:2:0', remainingHitPoints: 0, maximumHitPoints: 2, active: false
  });

  assert.deepEqual(change.removed.map(chunk => chunk.id),['wall:1:0','wall:2:0','wall:3:0','wall:1:1','wall:2:1','wall:3:1','wall:2:2']);
  assert.equal(structure.canOccupy([0,0.85,0],playerHalfExtents),true);
  const mesh = createChunkMeshData(structure.activeChunks());
  assert.equal(mesh.metadata?.chunkCount,8);
  assert.equal(mesh.indices.length/36,8);

  const batch = createRemovalMutationBatch(change.revision,change.removed.map(chunk => chunk.id));
  assert.equal(batch.invalidatesContacts,true);
  assert.equal(batch.rebuildsAccelerationStructure,true);
});

test('overlapping hits, raycasts, and reset retain stable identities without stale state', () => {
  const definitions = wall();
  const structure = new ChunkStructure(definitions);
  structure.queueDamage({center:[0,0.5,0],radius:0.4,damage:2});
  structure.queueDamage({center:[0,0.5,0],radius:0.4,damage:2});
  assert.deepEqual(structure.commit().removed.map(chunk => chunk.id),['wall:2:0']);
  assert.equal(structure.commit().removed.length,0);
  assert.equal(structure.raycast([0,0.5,4],[0,0,-1],10)?.chunk.id,undefined);

  structure.reset();
  assert.ok(structure.damageState().every(chunk => chunk.active && chunk.remainingHitPoints === chunk.maximumHitPoints));
  assert.deepEqual(structure.activeChunks().map(chunk => chunk.id),definitions.map(chunk => chunk.id));
  assert.equal(structure.raycast([0,0.5,4],[0,0,-1],10)?.chunk.id,'wall:2:0');
});

test('ground support releases healthy overhangs and detached islands at the damage boundary', () => {
  const structure = new ChunkStructure(wall(), {groundHeight: 0, maximumCantileverDistance: 1.05});
  assert.ok(structure.supportState().every(chunk => chunk.supported));
  for(const column of [1, 2, 3]) {
    for(const row of [0, 1]) {
      structure.queueDamage({center: [column - 2, row + 0.5, 0], radius: 0.1, damage: 2});
    }
  }
  assert.equal(structure.activeChunks().length, 15, 'Support and collision stay unchanged before commit');
  const opening = structure.commit();
  assert.deepEqual(opening.detached.map(chunk => chunk.id), ['wall:2:2']);
  assert.equal(structure.damageState().find(chunk => chunk.chunkId === 'wall:2:2').remainingHitPoints, 2, 'Loss of support does not fabricate impact damage');
  assert.equal(structure.raycast([0, 2.5, 4], [0, 0, -1], 10), null, 'Detached chunks leave static ray queries immediately');
  const bridge = structure.supportState().find(chunk => chunk.chunkId === 'wall:1:2');
  assert.equal(bridge.supported, true);
  assert.equal(bridge.lateralDistance, 1);
  bridge.lateralDistance = -99;
  assert.equal(structure.supportState().find(chunk => chunk.chunkId === bridge.chunkId).lateralDistance, 1);

  for(const x of [-2, 2]) {
    structure.queueDamage({center: [x, 0.5, 0], radius: 0.1, damage: 2});
  }
  const collapse = structure.commit();
  assert.equal(collapse.detached.length, 6, 'Removing both remaining feet releases the whole upper section');
  assert.equal(collapse.activeChunkCount, 0);
  assert.equal(structure.commit().removed.length, 0, 'Already released chunks cannot detach twice');
  structure.reset();
  assert.equal(structure.activeChunks().length, 15);
  assert.ok(structure.supportState().every(chunk => chunk.supported));
});

test('support excludes corner contacts and does not regain sideways reach by climbing', () => {
  const definitions = [
    {id: 'foot', center: [0, 0.5, 0]},
    {id: 'bridge', center: [1, 1.5, 0]},
    {id: 'above', center: [1, 2.5, 0]},
    {id: 'overhang', center: [2, 2.5, 0]},
    {id: 'corner', center: [-1, 2.5, 1]},
    {id: 'column', center: [0, 1.5, 0]}
  ].map(chunk => ({...chunk, size: [1, 1, 1], material: 'masonry', hitPoints: 2}));
  const structure = new ChunkStructure(definitions, {groundHeight: 0, maximumCantileverDistance: 1.05});
  assert.deepEqual(structure.commit().detached.map(chunk => chunk.id), ['overhang', 'corner']);

  const staggered = Array.from({length: 7}, (_, index) => ({
    id: `step:${index}`, center: [index*0.5, index + 0.5, 0],
    size: [1, 1, 1], material: 'masonry', hitPoints: 2
  }));
  const staircase = new ChunkStructure(staggered, {groundHeight: 0, maximumCantileverDistance: 1.05});
  assert.deepEqual(staircase.commit().detached.map(chunk => chunk.id), ['step:3', 'step:4', 'step:5', 'step:6'],
    'Offset vertical contacts must spend the same sideways allowance as lateral contacts');
});
