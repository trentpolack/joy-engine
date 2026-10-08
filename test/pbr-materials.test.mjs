// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { PbrMaterialSet } from '../src/rendering/pbr/pbr-materials.ts';

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

function bitmap(width = 16, height = 16) {
  return { width, height, closed: 0, close() { this.closed++; } };
}

function gpu() {
  const allocations = [];
  function allocate(properties) {
    const resource = { properties, destroyed: 0, destroy() { this.destroyed++; } };
    allocations.push(resource);
    return resource;
  }
  return { allocations, device: {
    limits: { maxTextureDimension2D: 2048 }, createTexture: allocate, createBuffer: allocate
  } };
}

function material(textures = {}) {
  return {
    baseColor: [0.5, 0.25, 0.75, 0.5], emissive: [2, 1, 0.5], metallic: 0.25,
    roughness: 0.75, alphaMode: 'BLEND', alphaCutoff: 0.5,
    textures: { baseColor: null, normal: null, emissive: null, metallic: null, roughness: null, ao: null, ...textures }
  };
}

function decodeWith(t, decode) {
  t.mock.method(globalThis, 'fetch', async (_source, options) => {
    if(options.signal.aborted) {
      throw new Error('aborted');
    }
    return new Response(new Blob(['image'], { type: 'image/png' }));
  });
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'createImageBitmap');
  Object.defineProperty(globalThis, 'createImageBitmap', { configurable: true, value: decode });
  t.after(() => {
    if(descriptor) {
      Object.defineProperty(globalThis, 'createImageBitmap', descriptor);
    } else {
      delete globalThis.createImageBitmap;
    }
  });
}

test('PBR material replacement shares images and preserves shader factor multiplication', async t => {
  const renderer = gpu();
  const materials = new PbrMaterialSet(renderer.device);
  t.after(() => materials.destroy());
  const images = [];
  decodeWith(t, async () => {
    const image = bitmap();
    images.push(image);
    return image;
  });
  assert.equal(await materials.update([
    material({ baseColor: 'image.png', emissive: 'image.png' }),
    material({ roughness: 'image.png' })
  ]), true);
  const original = materials.bindings;
  const texture = original[0].baseColorTexture;
  assert.equal(texture, original[0].emissiveTexture);
  assert.equal(texture, original[1].roughnessTexture);
  assert.equal(images.length, 1);
  assert.deepEqual([...original[0].uniforms.properties.data], [0.5, 0.25, 0.75, 0.5, 2, 1, 0.5, 0.25, 0.75, 2, 0.5, 0]);
  assert.equal(texture.properties.format, 'rgba8unorm');
  assert.equal(await materials.update([material({ baseColor: 'image.png' })]), true);
  assert.equal(materials.bindings[0].baseColorTexture, texture);
  assert.equal(texture.destroyed, 0);
  assert.ok(original.every(binding => binding.uniforms.destroyed === 1));
  assert.equal(images.length, 1);
  assert.equal(images[0].closed, 1);
  await materials.update([]);
  assert.equal(texture.destroyed, 1);
  materials.destroy();
  materials.destroy();
  assert.ok(renderer.allocations.every(resource => resource.destroyed === 1));
});

test('failed PBR replacement retains drawable materials and releases partial allocations', async t => {
  const renderer = gpu();
  const materials = new PbrMaterialSet(renderer.device);
  t.after(() => materials.destroy());
  let decodes = 0;
  decodeWith(t, async () => {
    if(++decodes === 3) {
      throw new Error('invalid image');
    }
    return bitmap();
  });
  await materials.update([material({ baseColor: 'active.png' })]);
  const active = materials.bindings;
  const previousAllocations = renderer.allocations.length;
  await assert.rejects(materials.update([material({ baseColor: 'next.png', normal: 'broken.png' })]), /invalid image/);
  assert.equal(materials.bindings, active);
  assert.equal(active[0].baseColorTexture.destroyed, 0);
  assert.equal(active[0].uniforms.destroyed, 0);
  assert.ok(renderer.allocations.slice(previousAllocations).every(resource => resource.destroyed === 1));
});

test('failed PBR uniform allocation rolls back complete pending material resources', async t => {
  const renderer = gpu();
  const materials = new PbrMaterialSet(renderer.device);
  t.after(() => materials.destroy());
  decodeWith(t, async () => bitmap());
  await materials.update([material({ baseColor: 'active.png' })]);
  const active = materials.bindings;
  const previousAllocations = renderer.allocations.length;
  const createBuffer = renderer.device.createBuffer;
  let buffers = 0;
  renderer.device.createBuffer = properties => {
    if(++buffers === 2) {
      throw new Error('GPU allocation failed');
    }
    return createBuffer(properties);
  };
  await assert.rejects(materials.update([
    material({ baseColor: 'next.png' }), material({ normal: 'next.png' })
  ]), /GPU allocation failed/);
  assert.equal(materials.bindings, active);
  assert.equal(active[0].uniforms.destroyed, 0);
  assert.equal(active[0].baseColorTexture.destroyed, 0);
  assert.ok(renderer.allocations.slice(previousAllocations).every(resource => resource.destroyed === 1));
});

test('PBR owner commit cancellation or failure preserves previous drawable bindings', async t => {
  const renderer = gpu();
  const materials = new PbrMaterialSet(renderer.device);
  t.after(() => materials.destroy());
  await materials.update([material()]);
  const active = materials.bindings;
  const previousAllocations = renderer.allocations.length;
  assert.equal(await materials.update([material()], () => false), false);
  await assert.rejects(materials.update([material()], () => {
    throw new Error('owner commit failed');
  }), /owner commit failed/);
  assert.equal(materials.bindings, active);
  assert.equal(active[0].uniforms.destroyed, 0);
  assert.ok(renderer.allocations.slice(previousAllocations).every(resource => resource.destroyed === 1));
});

for(const action of ['supersede', 'dispose']) {
  test(`${action} cancels PBR preparation and releases pending images`, async t => {
    const renderer = gpu();
    const materials = new PbrMaterialSet(renderer.device);
    t.after(() => materials.destroy());
    const decoding = deferred();
    const started = deferred();
    const images = [bitmap(), bitmap()];
    let decodes = 0;
    decodeWith(t, async () => {
      if(++decodes === 1) {
        return images[0];
      }
      started.resolve();
      return decoding.promise;
    });
    const preparing = materials.update([material({ baseColor: 'first.png', normal: 'second.png' })]);
    await started.promise;
    if(action === 'supersede') {
      assert.equal(await materials.update([material()]), true);
    } else {
      materials.destroy();
    }
    decoding.resolve(images[1]);
    assert.equal(await preparing, false);
    assert.ok(images.every(image => image.closed === 1));
    materials.destroy();
    assert.ok(renderer.allocations.every(resource => resource.destroyed === 1));
    assert.equal(await materials.update([material()]), false);
  });
}

test('oversized PBR images close before GPU allocation and preserve active materials', async t => {
  const renderer = gpu();
  const materials = new PbrMaterialSet(renderer.device);
  t.after(() => materials.destroy());
  await materials.update([material()]);
  const active = materials.bindings;
  const image = bitmap(2049, 1);
  decodeWith(t, async () => image);
  const previousAllocations = renderer.allocations.length;
  await assert.rejects(materials.update([material({ normal: 'oversized.png' })]), /2048/);
  assert.equal(materials.bindings, active);
  assert.equal(renderer.allocations.length, previousAllocations);
  assert.equal(image.closed, 1);
});

test('invalid PBR factors fail before allocating and preserve active materials', async t => {
  const renderer = gpu();
  const materials = new PbrMaterialSet(renderer.device);
  t.after(() => materials.destroy());
  await materials.update([material()]);
  const active = materials.bindings;
  const previousAllocations = renderer.allocations.length;
  for(const invalid of [
    { roughness: -0.1 }, { metallic: 1.1 }, { alphaCutoff: NaN },
    { baseColor: [1, 1, 1] }, { baseColor: [1, 1, 1, 2] },
    { emissive: [1, -1, 0] }, { emissive: [1, 0, 1e100] }, { alphaMode: 'unknown' },
    { textures: { ...material().textures, normal: 42 } }
  ]) {
    await assert.rejects(materials.update([{ ...material(), ...invalid }]), /PBR material/);
  }
  assert.equal(materials.bindings, active);
  assert.equal(renderer.allocations.length, previousAllocations);
});
