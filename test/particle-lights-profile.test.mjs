import test from 'node:test';
import assert from 'node:assert/strict';
import { compileParticleEffect } from '../src/particles/asset.ts';
import { ParticleEffect } from '../src/particles/particle-effect.ts';
import { collectParticleLights } from '../src/particles/lights.ts';

function document(overrides = {}) {
  return { version: 1, name: 'Metrics', maxParticles: 64, parameters: {}, emitters: [{
    id: 'main', enabled: true, root: true, duration: 0, rate: 0,
    bursts: [{ time: 0, count: 2 }], lifetime: 1, renderer: 'square',
    spawn: 'vx = rand(1, 2); r = 2; g = 1; b = 0.5;', update: 'alpha = 1 - t;', ...overrides,
  }] };
}
function runtime(source, options) {
  return new ParticleEffect(compileParticleEffect(source), options);
}

test('lights validate assets, expose writable fields and preserve simulations', () => {
  for(const light of [null, {}, { intensity: -1, range: 20 }, { intensity: 1, range: 0 }, { intensity: 1001, range: 20 }, { intensity: 1, range: Infinity }]) {
    assert.throws(() => compileParticleEffect(document({ light })), /light/);
  }
  const effect = runtime(document({ light: { intensity: 4, range: 20 }, spawn: 'x=2;y=3;z=4;r=2;g=1;b=0.5;alpha=0.5;', update: 'lightIntensity = 2; lightRange = 12;' }));
  const before = JSON.stringify(effect.particles);
  assert.deepEqual(collectParticleLights(effect).lights[0], { x: 2, y: 3, z: 4, r: 4, g: 2, b: 1, range: 20, emitterIndex: 0, particleIndex: 0 });
  assert.equal(JSON.stringify(effect.particles), before);
  effect.step();
  assert.equal(collectParticleLights(effect).lights[0].range, 12);
  assert.equal(collectParticleLights(effect).lights[0].r, 2);
  const unlit = runtime(document({ spawn: 'lightIntensity = 4; lightRange = 20;' }));
  assert.equal(collectParticleLights(unlit).lights.length, 0);
});

test('lights select strongest 32 with stable ties, and filter dark, retired and invalid lights', () => {
  const effect = runtime(document({ bursts: [{ time: 0, count: 40 }], light: { intensity: 1, range: 20 }, spawn: 'r = index; g = 0; b = 0;' }));
  const selected = collectParticleLights(effect, 100);
  assert.equal(selected.lights.length, 32);
  assert.equal(selected.omitted, 7);
  assert.deepEqual(selected.lights.map(light => light.particleIndex), Array.from({ length: 32 }, (_, i) => 39 - i));
  assert.equal(collectParticleLights(effect, 0).omitted, 39);
  assert.throws(() => collectParticleLights(effect, NaN));
  const ties = runtime(document({ light: { intensity: 1, range: 20 } }));
  assert.deepEqual(collectParticleLights(ties).lights.map(light => light.particleIndex), [0, 1]);
  for(const spawn of ['lightIntensity=0;', 'lightRange=0;', 'alpha=0;', 'lifetime=0;']) {
    assert.equal(collectParticleLights(runtime(document({ light: { intensity: 1, range: 20 }, spawn }))).lights.length, 0);
  }
  effect.particles[1].r = NaN;
  assert.equal(collectParticleLights(effect, 0).omitted, 38);
  ties.seek(1);
  assert.equal(collectParticleLights(ties).lights.length, 0);
  effect.destroy();
  assert.deepEqual(collectParticleLights(effect), { lights: [], omitted: 0 });
});

test('profiling preserves seeded trajectories and avoids clock calls when disabled', () => {
  let clock = 0;
  const source = document({ duration: 2, rate: 37 });
  const plain = runtime(source, { seed: 31, profileClock: () => { throw Error('clock called'); } });
  const profiled = runtime(source, { seed: 31, profiling: true, profileClock: () => ++clock });
  for(let i = 0; i < 24; i++) {
    plain.step();
    profiled.step();
  }
  assert.deepEqual(plain.particles, profiled.particles);
  assert.equal(profiled.profile.steps, 24);
  assert.ok(profiled.profile.simulationMs > 0);
  assert.ok(profiled.profile.emitters[0].spawnMs > 0);
  assert.ok(profiled.profile.emitters[0].updateMs > 0);
  assert.equal(profiled.profile.emitters[0].live, profiled.particles.length);
  profiled.seek(0.4);
  assert.deepEqual(plain.particles, profiled.particles);
  profiled.reset();
  assert.equal(profiled.profile.steps, 0);
  assert.equal(profiled.profile.emitters[0].spawned, 2);
  assert.equal(profiled.profile.emitters[0].live, 2);
  assert.equal(profiled.profile.emitters[0].updateMs, 0);
  assert.ok(profiled.profile.simulationMs > 0);
  profiled.destroy();
  assert.equal(profiled.profile.emitters[0].live, 0);
});

test('profile counts root and child drops and actual script operations', () => {
  const source = document({ lifetime: 1 / 60, spawn: 'x=1;', update: 'x+=1;', death: { emitter: 'child', count: 3 } });
  source.maxParticles = 2;
  source.emitters.push({ ...source.emitters[0], id: 'child', root: false, death: undefined, lifetime: 1 });
  const effect = runtime(source, { profiling: true });
  const main = effect.profile.emitters[0];
  assert.equal(main.spawned, 2);
  assert.equal(main.operations, 2);
  effect.step();
  assert.equal(main.live, 0);
  assert.equal(main.operations, 4);
  assert.equal(effect.profile.emitters[1].spawned, 2);
  assert.equal(effect.profile.emitters[1].live, 2);
  assert.equal(effect.profile.emitters[1].dropped, 4);
  assert.equal(effect.droppedParticles, 4);
  const overfull = document({ bursts: [{ time: 0, count: 3 }] });
  overfull.maxParticles = 2;
  assert.equal(runtime(overfull, { profiling: true }).profile.emitters[0].dropped, 1);
});
