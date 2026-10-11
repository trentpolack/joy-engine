// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import {prepareShadowLightBatches} from '../src/rendering/shadows/shadow-light-batches.ts';

test('shadow batches retain every direct light and allocate only casting slots', () => {
  const lights = Array.from({length: 32}, (_, index) => ({x: index, y: 2, z: 0, r: 1, g: 1, b: 1, range: 8}));
  lights[1].castsShadow = false;
  const batches = prepareShadowLightBatches(lights);
  assert.equal(batches.length, 8);
  assert.deepEqual(batches.flatMap(batch => batch.lights), lights);
  assert.deepEqual(batches.flatMap(batch => batch.indices), lights.map((_, index) => index));
  assert.deepEqual(batches[0].shadowIndices, [0, -1, 1, 2]);
  assert.ok(batches.every(batch => batch.shadowIndices.every(index => index < 4)));
  assert.equal(prepareShadowLightBatches([]).length, 1, 'directional-only scenes still have a base batch');
  assert.throws(() => prepareShadowLightBatches([...lights, lights[0]]), /32/);
});
