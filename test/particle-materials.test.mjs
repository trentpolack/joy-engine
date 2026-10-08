// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { compileParticleEffect } from '../src/particles/asset.ts';
import { ParticleEffect } from '../src/particles/particle-effect.ts';
import { appendParticleEffect, createParticleEffectBatches, PARTICLE_XY_BASIS } from '../src/particles/rendering.ts';
import { DEFAULT_PARTICLE_TEXTURE, readParticleTextureSource } from '../src/particles/texture-source.ts';

function asset(renderer = 'soft', texture) {
  const emitter = { id: 'one', enabled: true, root: true, duration: 0, rate: 0,
    bursts: [{ time: 0, count: 1 }], lifetime: 2, renderer,
    spawn: 'size = 2; alpha = 0.5;', update: '' };
  if(texture !== undefined) {
    emitter.texture = texture;
  }
  return { version: 1, name: 'Materials', maxParticles: 16, parameters: {}, emitters: [emitter] };
}
const texture = (options = {}) => ({ source: DEFAULT_PARTICLE_TEXTURE, ...options });
const runtime = (renderer, options) => new ParticleEffect(compileParticleEffect(asset(renderer, options)));
const batches = effect => createParticleEffectBatches(effect, PARTICLE_XY_BASIS);

test('texture normalization owns and freezes data while old assets retain absent texture', () => {
  const input = asset('flipbook', texture({ columns: 2, rows: 2 }));
  const result = compileParticleEffect(input);
  assert.deepEqual(result.asset.emitters[0].texture, texture({ columns: 2, rows: 2, frames: 4, fps: 0, loop: true }));
  assert.deepEqual(result.emitters[0].textureDimensions, { width: 16, height: 16 });
  assert.ok(Object.isFrozen(result.emitters[0].textureDimensions));
  assert.ok(!('textureDimensions' in result.asset.emitters[0]));
  input.emitters[0].texture.columns = 8;
  assert.equal(result.asset.emitters[0].texture.columns, 2);
  assert.throws(() => {
    result.asset.emitters[0].texture.columns = 8;
  }, TypeError);
  assert.ok(!('texture' in compileParticleEffect(asset()).asset.emitters[0]));
  assert.deepEqual(readParticleTextureSource(DEFAULT_PARTICLE_TEXTURE), { width: 16, height: 16 });
});

test('texture schema rejects unsupported sources, invalid bounds and missing materials', () => {
  for(const renderer of ['textured', 'flipbook']) {
    assert.throws(() => compileParticleEffect(asset(renderer)), /requires a texture/);
  }
  for(const config of [texture({ source: 'https://example.com/image.png' }), texture({ source: 'data:image/png;base64,AAAA' }), texture({ columns: 17 }), texture({ rows: 0 }), texture({ frames: 2 }), texture({ fps: 121 }), texture({ loop: 'true' })]) {
    assert.throws(() => compileParticleEffect(asset('textured', config)));
  }
  const bytes = Buffer.from(DEFAULT_PARTICLE_TEXTURE.split(',')[1], 'base64');
  bytes.writeUInt32BE(2049, 16);
  assert.throws(() => readParticleTextureSource(`data:image/png;base64,${bytes.toString('base64')}`), /dimensions/);
  assert.throws(() => readParticleTextureSource(`data:image/png;base64,${Buffer.alloc(1024 * 1024 + 1).toString('base64')}`), /1 MiB/);
});

test('effect texture budgets reject excessive distinct sources and repeated encoded data', () => {
  const png = Buffer.from(DEFAULT_PARTICLE_TEXTURE.split(',')[1], 'base64');
  const input = asset('textured', texture());
  input.emitters = Array.from({ length: 9 }, (_, index) => ({
    ...input.emitters[0], id: `sprite${index}`,
    texture: texture({ source: `data:image/png;base64,${Buffer.concat([png, Buffer.from([index])]).toString('base64')}` }),
  }));
  assert.throws(() => compileParticleEffect(input), /8 distinct/);
  input.emitters.pop();
  assert.equal(compileParticleEffect(input).emitters.length, 8);
  const large = `data:image/png;base64,${Buffer.concat([png, Buffer.alloc(850000)]).toString('base64')}`;
  input.emitters = input.emitters.slice(0, 4).map(emitter => ({ ...emitter, texture: texture({ source: large }) }));
  assert.throws(() => compileParticleEffect(input), /4 MiB/);
});

test('flipbook frames use seconds, row-major UVs, clamp, loop and normalized lifetime', () => {
  for(const [options, age, expected] of [
    [{ fps: 4, loop: false }, 1.5, [0.53125, 0.53125]],
    [{ fps: 4, loop: true }, 1.5, [0.03125, 0.53125]],
    [{ fps: 0 }, 1.25, [0.03125, 0.53125]],
  ]) {
    const effect = runtime('flipbook', texture({ columns: 2, rows: 2, ...options }));
    effect.particles[0].age = age;
    const batch = batches(effect)[0];
    assert.equal(batch.vertices.length, 54);
    assert.deepEqual(batch.vertices.slice(7, 9), expected);
    assert.equal(batch.source, DEFAULT_PARTICLE_TEXTURE);
  }
  const effect = runtime('textured', texture({ columns: 2, rows: 2, fps: 4 }));
  effect.particles[0].age = 1.5;
  assert.deepEqual(batches(effect)[0].vertices.slice(7, 9), [0.03125, 0.03125]);
  assert.throws(() => appendParticleEffect([], effect, PARTICLE_XY_BASIS), /createParticleEffectBatches/);
});

test('mixed material runs keep global depth order and deterministic read-only rendering', () => {
  const input = asset('textured', texture());
  input.emitters.push({ ...asset('square').emitters[0], id: 'two' });
  input.emitters.push({ ...input.emitters[0], id: 'three' });
  const effect = new ParticleEffect(compileParticleEffect(input));
  effect.particles[0].z = 10;
  effect.particles[1].z = 0;
  effect.particles[2].z = -10;
  const before = JSON.stringify(effect.particles);
  const first = batches(effect);
  assert.deepEqual(first.map(batch => batch.vertices[2]), [-10, 0, 10]);
  assert.deepEqual(first.map(batch => batch.source), [DEFAULT_PARTICLE_TEXTURE, undefined, DEFAULT_PARTICLE_TEXTURE]);
  assert.deepEqual(batches(effect), first);
  assert.equal(JSON.stringify(effect.particles), before);
  const reverse = createParticleEffectBatches(effect, { ...PARTICLE_XY_BASIS, backward: [0, 0, -1] });
  assert.deepEqual(reverse.map(batch => batch.vertices[2]), [10, 0, -10]);
  // Editor reference lines share the same depth order, including textured runs.
  const reference = new Float32Array([
    -1, -1, 5, 0.2, 0.3, 0.4, 0.5,
    1, -1, 5, 0.2, 0.3, 0.4, 0.5,
    0, 1, 5, 0.2, 0.3, 0.4, 0.5
  ]);
  const referenceBefore = reference.slice();
  for(const backward of [[0, 0, 1], [0, 0, -1]]) {
    const mixed = createParticleEffectBatches(effect, {...PARTICLE_XY_BASIS, backward}, undefined, reference);
    const depths = mixed.flatMap(batch => {
      const stride = batch.source ? 9 : 7;
      const values = [];
      for(let offset = 0; offset < batch.vertices.length; offset+= stride*3) {
        values.push(batch.vertices[offset + 2]*backward[2]);
      }
      return values;
    });
    assert.deepEqual(depths, [...depths].sort((a, b) => a - b));
    assert(depths.includes(5*backward[2]));
  }
  assert.deepEqual(reference, referenceBefore);
  assert.equal(JSON.stringify(effect.particles), before);
  assert.throws(() => createParticleEffectBatches(effect, PARTICLE_XY_BASIS, undefined, [0]), RangeError);
  effect.particles[1].z = 20;
  assert.equal(batches(effect).length, 2, 'adjacent identical materials merge');
});
