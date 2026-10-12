// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import {ChunkStructure} from 'joy-engine';
import {generateStructure} from '../src/structure.ts';
import {DESTRUCTION_CONFIG} from '../src/config/destruction-config.ts';

test('identical seeds reproduce stable structure and reset restores collision', () => {
  assert.deepEqual(generateStructure(42),generateStructure(42));
  assert.notDeepEqual(generateStructure(42),generateStructure(43));
  const structure = new ChunkStructure(generateStructure(42));
  for(let hit = 0; hit < DESTRUCTION_CONFIG.hitPoints; hit++) {
    structure.queueDamage({center:[0,1,0],radius:1.3,damage:1});
    structure.commit();
  }
  const queryHalfExtents = [0.34,0.82,0.34];
  assert.equal(structure.canOccupy([0,0.9,0],queryHalfExtents),true);
  structure.reset();
  assert.equal(structure.canOccupy([0,0.9,0],queryHalfExtents),false);
});

test('released chunks land promptly, persist without eviction, and release on reset', async () => {
  const {RenderScene} = await import('joy-engine');
  const {DestructionDebris} = await import('../src/destruction-debris.ts');
  const scene = new RenderScene();
  const debris = new DestructionDebris(scene);
  const chunks = generateStructure();
  const chunk = {...chunks[0], center: [0, 4, 0]};
  try {
    debris.spawn([chunk]);
    for(let frame = 0; frame < 60; frame++) {
      debris.update(1/60);
      assert.ok(debris.snapshot().pieces.every(piece => piece.position[1] > 0.2), 'Debris must not fall through the ground');
    }
    const landed = debris.snapshot().pieces[0];
    assert.ok(landed.position[1] < 1, 'A chunk released four meters up should reach the ground within one second');
    for(let frame = 0; frame < 60*20; frame++) {
      debris.update(1/60);
    }
    assert.equal(debris.count, 1, 'Debris persists beyond the former lifetime');
    assert.ok(Math.hypot(...debris.snapshot().pieces[0].velocity) < 0.15, 'Ground friction must stop debris sliding');

    debris.spawn(chunks);
    debris.spawn(chunks);
    assert.equal(debris.count, chunks.length, 'All structure chunks persist; duplicate releases allocate nothing');
    assert.equal(scene.instances.size, chunks.length + 1);
    debris.reset();
    assert.equal(debris.snapshot().pieces.length, 0);
    assert.equal(scene.instances.size, 1, 'Reset keeps the ground and releases every debris mesh');
    debris.spawn([chunk]);
    assert.equal(debris.count, 1, 'A reset permits the same chunk to be released again');
  } finally {
    debris.dispose();
    debris.dispose();
    assert.equal(scene.instances.size, 0);
    scene.destroy();
  }
});

test('impact debug drawing bounds and releases scene resources across visibility and lifetime changes', async () => {
  const {RenderScene} = await import('joy-engine');
  const {ImpactDebugDraw} = await import('../src/impact-debug.ts');
  const {IMPACT_DEBUG_CONFIG} = await import('../src/config/destruction-config.ts');
  const scene = new RenderScene();
  const debug = new ImpactDebugDraw(scene);
  try {
    for(let index = 0; index < IMPACT_DEBUG_CONFIG.maximumMarkers + 5; index++) {
      debug.add([index, 2, 0], [0, 0, -1], 1);
    }
    assert.equal(scene.instances.size, IMPACT_DEBUG_CONFIG.maximumMarkers);
    debug.setEnabled(false);
    debug.add([0, 2, 0], [0, 0, -1], 1);
    assert.equal(scene.instances.size, 0, 'Disabled diagnostics release existing meshes and allocate no new ones');

    debug.setEnabled(true);
    debug.add([0, 2, 0], [0, 0, -1], 1);
    assert.equal(scene.instances.size, 1);
    debug.update(IMPACT_DEBUG_CONFIG.lifetimeSeconds);
    assert.equal(scene.instances.size, 0, 'Expired markers release their scene references');
    debug.add([0, 2, 0], [0, 0, -1], 1);
    debug.clear();
    assert.equal(scene.instances.size, 0);
    debug.add([0, 2, 0], [0, 0, -1], 1);
  } finally {
    debug.dispose();
    debug.dispose();
    assert.equal(scene.instances.size, 0);
    scene.destroy();
  }
});

test('support-released chunks fall onto surviving structure and drop when its collider is removed', async () => {
  const {RenderScene} = await import('joy-engine');
  const {DestructionDebris} = await import('../src/destruction-debris.ts');
  const scene = new RenderScene();
  const debris = new DestructionDebris(scene);
  const foot = {id: 'foot', center: [0, 0.5, 0], size: [1, 1, 1], material: 'masonry', hitPoints: 6};
  const upper = {...foot, id: 'upper', center: [0, 4, 0]};
  try {
    debris.synchronizeStructure([foot]);
    debris.spawn([upper], false);
    assert.deepEqual(debris.snapshot().pieces[0].velocity, [0, 0, 0], 'Support failures fall without an explosion impulse');
    for(let frame = 0; frame < 180; frame++) {
      debris.update(1/60);
    }
    assert.ok(debris.snapshot().pieces[0].position[1] > 1.4, 'The surviving wall catches the falling chunk');
    assert.ok(debris.snapshot().pieces[0].position[1] < 1.6);
    debris.synchronizeStructure([]);
    for(let frame = 0; frame < 180; frame++) {
      debris.update(1/60);
    }
    assert.ok(debris.snapshot().pieces[0].position[1] < 0.6, 'Removing the static collider allows the chunk to reach the ground');
    assert.equal(debris.snapshot().staticChunks, 0);
    debris.reset();
    debris.synchronizeStructure([foot]);
    assert.equal(debris.snapshot().staticChunks, 1);
    assert.equal(debris.count, 0);

    const {SUPPORT_CONFIG} = await import('../src/config/destruction-config.ts');
    const structure = new ChunkStructure(generateStructure(), SUPPORT_CONFIG);
    for(let hit = 0; hit < DESTRUCTION_CONFIG.hitPoints; hit++) {
      structure.queueDamage({center: [-2, 2.5, 0.275], radius: 1.15, damage: 1});
    }
    const changes = structure.commit();
    const detached = new Set(changes.detached.map(chunk => chunk.id));
    debris.synchronizeStructure(structure.activeChunks());
    debris.spawn(changes.removed.filter(chunk => !detached.has(chunk.id)));
    debris.spawn(changes.detached, false);
    for(let frame = 0; frame < 180; frame++) {
      debris.update(1/60);
    }
    assert.ok(debris.snapshot().pieces.some(piece => piece.position[2] > 0.5 && piece.position[1] < 1), 'Fractured chunks must escape the wall instead of wedging between intact neighbors');
  } finally {
    debris.dispose();
    assert.equal(scene.instances.size, 0);
    scene.destroy();
  }
});
