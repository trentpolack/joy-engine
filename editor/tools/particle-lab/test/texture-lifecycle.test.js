// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { GpuTriangleRenderer, ParticleTextureSet, DEFAULT_PARTICLE_TEXTURE, compileParticleEffect, ParticleEffect, OrbitCamera } from 'joy-engine';
import { ParticleViewport } from '../src/viewport.ts';

function deferred() {
  let resolve;
  const promise = new Promise(done => {
    resolve = done;
  });
  return { promise, resolve };
}

function installGlobal(t, name, value) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  t.after(() => {
    if(descriptor) {
      Object.defineProperty(globalThis, name, descriptor);
    } else {
      delete globalThis[name];
    }
  });
}

function bitmap() {
  return { width: 16, height: 16, closed: 0, close() {
    this.closed++;
  } };
}

function gpu() {
  const allocations = [];
  const device = { createTexture() {
    const texture = { destroyed: 0, destroy() {
      this.destroyed++;
    } };
    allocations.push(texture);
    return texture;
  } };
  return { device, allocations };
}

// Distinct embedded PNG sources; trailing bytes do not change their image header.
const sources = [0, 1, 2].map(index => `data:image/png;base64,${Buffer.concat([
  Buffer.from(DEFAULT_PARTICLE_TEXTURE.split(',')[1], 'base64'), Buffer.from([index]),
]).toString('base64')}`);
const definition = (...images) => ({ emitters: images.map(source => ({ renderer: 'textured', texture: { source } })) });

test('initial texture decoding keeps resize renders safe until images are ready', async t => {
  const decoding = deferred();
  const started = deferred();
  const image = bitmap();
  installGlobal(t, 'createImageBitmap', () => {
    started.resolve();
    return decoding.promise;
  });
  installGlobal(t, 'location', { search: '' });
  installGlobal(t, 'devicePixelRatio', 1);
  const renderer = { ...gpu(), backend: 'webgl', resize() {}, render: t.mock.fn() };
  t.mock.method(GpuTriangleRenderer, 'create', async () => renderer);
  const effect = new ParticleEffect(compileParticleEffect({
    version: 1, name: 'Startup texture', maxParticles: 16, parameters: {},
    emitters: [{ id: 'sprite', renderer: 'textured', texture: { source: sources[0] },
      enabled: true, root: true, duration: 0, rate: 0, bursts: [{ time: 0, count: 1 }],
      lifetime: 2, spawn: 'size = 1;', update: '' }],
  }));
  const errors = [];
  const viewport = Object.assign(Object.create(ParticleViewport.prototype), {
    renderer: null, textures: null, texturesReady: false, disposed: false,
    canvas: { clientWidth: 64, clientHeight: 64, dataset: {} }, camera: new OrbitCamera(),
    gridVisible: false, receiverVisible: false, onError: error => errors.push(error),
  });
  viewport.setEffect(effect);
  const initializing = viewport.initialize();
  await started.promise;
  viewport.render();
  assert.deepEqual(errors, []);
  assert.equal(renderer.render.mock.calls.at(-1).arguments[5].length, 0);
  decoding.resolve(image);
  await initializing;
  assert.deepEqual(errors, []);
  assert.equal(renderer.render.mock.calls.at(-1).arguments[5].length, 1);
  assert.equal(image.closed, 1);
  viewport.textures.destroy();
  effect.destroy();
});

test('failed decoding preserves active textures and releases partial allocations', async t => {
  const renderer = gpu();
  const textures = new ParticleTextureSet(renderer);
  t.after(() => textures.destroy());
  let decodes = 0;
  const images = [];
  installGlobal(t, 'createImageBitmap', async () => {
    if(++decodes === 3) {
      throw new Error('invalid PNG pixels');
    }
    const image = bitmap();
    images.push(image);
    return image;
  });
  const active = definition(sources[0]);
  await textures.prepare(active);
  textures.activate(active);
  const retained = textures.get(sources[0]);
  await assert.rejects(textures.prepare(definition(sources[1], sources[2])), /invalid PNG pixels/);
  assert.equal(textures.get(sources[0]), retained);
  assert.equal(retained.destroyed, 0);
  assert.deepEqual(renderer.allocations.map(texture => texture.destroyed), [0, 1]);
  assert.ok(images.every(image => image.closed === 1));
});

for(const action of ['supersede', 'dispose']) {
  test(`${action} during decoding releases obsolete GPU textures and decoded images`, async t => {
    const renderer = gpu();
    const textures = new ParticleTextureSet(renderer);
    t.after(() => textures.destroy());
    const started = deferred();
    const decoding = deferred();
    const images = [bitmap(), bitmap()];
    let decodes = 0;
    installGlobal(t, 'createImageBitmap', async () => {
      if(++decodes === 1) {
        return images[0];
      }
      started.resolve();
      return decoding.promise;
    });
    const preparing = textures.prepare(definition(sources[0], sources[1]));
    await started.promise;
    if(action === 'supersede') {
      assert.equal(await textures.prepare(definition()), true);
    } else {
      textures.destroy();
    }
    decoding.resolve(images[1]);
    assert.equal(await preparing, false);
    assert.equal(textures.textures.size, 0);
    assert.deepEqual(renderer.allocations.map(texture => texture.destroyed), [1]);
    assert.ok(images.every(image => image.closed === 1));
  });
}
