// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { MAX_ASSET_BYTES, readAsset, writeAsset } from '../src/document.ts';
import { copyExample } from '../src/examples.ts';

test('authored assets survive a fully validated portable JSON round trip', () => {
  const asset = copyExample(0);
  assert.deepEqual(readAsset(writeAsset(asset)), asset);
});

test('readAsset delegates malformed nested records to shared validation', () => {
  const asset = copyExample(0);
  asset.emitters[0] = /** @type {never} */ (null);
  assert.throws(() => readAsset(writeAsset(asset)), /emitter|object/i);
});

test('readAsset rejects wrong versions and bounded-size violations', () => {
  assert.throws(() => readAsset('{"version":2}'), /version.*1/i);
  assert.throws(() => readAsset(' '.repeat(MAX_ASSET_BYTES + 1)), /cannot exceed/);
});

test('copyExample detaches mutable child records', () => {
  const first = copyExample(0);
  first.emitters[0].bursts[0].count = 1;
  assert.notEqual(copyExample(0).emitters[0].bursts[0].count, 1);
});

test('opening an asset preserves authored script whitespace exactly', () => {
  const asset = copyExample(0);
  asset.emitters[0].spawn = 'let speed = 4;\n\n  vx = speed; // intentional alignment\n';
  const opened = readAsset(writeAsset(asset));
  assert.equal(opened.emitters[0].spawn, asset.emitters[0].spawn);
});

test('vector properties round trip and expose XYZ script values', async () => {
  const { ParticleEffect, compileParticleEffect } = await import('joy-engine');
  const asset = copyExample(0);
  asset.parameters.direction = { type: 'vector3', value: [1, 2, 3], min: [-10, -10, -10], max: [10, 10, 10] };
  asset.emitters[0].spawn = 'vx = directionX; vy = directionY; vz = directionZ; drag = 0; ax = 0; ay = 0; az = 0;';
  asset.emitters[0].update = '';
  delete asset.emitters[0].trail;
  delete asset.emitters[0].death;
  asset.emitters = [asset.emitters[0]];
  const opened = readAsset(writeAsset(asset));
  const effect = new ParticleEffect(compileParticleEffect(opened), { parameters: { direction: [4, 5, 6] } });
  effect.update(0.2);
  assert.ok(effect.particles.every(particle => particle.vx === 4 && particle.vy === 5 && particle.vz === 6));
});
