// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import test from 'node:test';
import assert from 'node:assert/strict';
import { Buffer } from '@luma.gl/core';
import { SceneVolumeLights } from '../src/rendering/scene/scene-volume-lights.ts';

const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
const projection = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, -100/99, -1, 0, 0, -100/99, 0];
const camera = {viewMatrix: identity, projectionMatrix: projection,
  nearPlane: 1, farPlane: 100, width: 256, height: 256};
const light = {x: 2, y: 2, z: -4, range: 0.25, r: 3, g: 2, b: 1};

function createDevice() {
  const buffers = [];
  return {
    type: 'webgpu',
    limits: {maxBufferSize: 1024*1024, maxStorageBufferBindingSize: 1024*1024},
    buffers,
    allocationCount: 0,
    failAllocation: 0,
    destroyCount: 0,
    destroy() { this.destroyCount++; },
    createBuffer(props) {
      this.allocationCount++;
      if(this.allocationCount === this.failAllocation) {
        throw new Error('Allocation failed');
      }
      const storage = {
        ...props, data: null, destroyCount: 0,
        write(data) { this.data = data.slice(); },
        destroy() { this.destroyCount++; }
      };
      buffers.push(storage);
      return storage;
    }
  };
}

test('volume-light storage packs real view-space lights and camera-dependent logarithmic clusters', () => {
  const device = createDevice();
  const owner = new SceneVolumeLights(device, {clusterCountZ: 2});
  const lights = [light, {...light, x: 20, y: 20, z: -40}, {...light, x: -2, y: -2}];
  const frame = owner.update(lights, camera);
  assert.deepEqual(Array.from(frame.pointLights.data.slice(0, 8)), [2, 2, -4, 0.25, 3, 2, 1, 1]);
  assert.deepEqual(Array.from(frame.clusterLightCounts.data), [0, 1, 1, 0, 0, 1, 0, 0]);
  assert.equal(frame.clusterLightIndices.data[1*8], 0);
  assert.equal(frame.clusterLightIndices.data[2*8], 2);
  assert.equal(frame.clusterLightIndices.data[5*8], 1);
  assert.ok(device.buffers.every(buffer => buffer.usage === (Buffer.STORAGE | Buffer.COPY_DST)));

  const movedView = [...identity];
  movedView[12] = -8;
  const moved = owner.update([light], {...camera, viewMatrix: movedView});
  assert.equal(moved.pointLights, frame.pointLights);
  assert.deepEqual(Array.from(moved.pointLights.data.slice(0, 8)), [-6, 2, -4, 0.25, 3, 2, 1, 1]);
  assert.ok(moved.clusterLightCounts.data.every(count => count === 0));
  assert.deepEqual(lights[0], light);
  const empty = owner.update([], camera);
  assert.equal(empty.pointLightCount, 0);
  assert.ok(empty.pointLights.data.every(value => value === 0));
  owner.destroy();
});

test('volume-light clustering bounds global and per-cluster work and reports deterministic overflow', () => {
  const owner = new SceneVolumeLights(createDevice(), {maxPointLights: 2, maxLightsPerCluster: 1, clusterCountZ: 2});
  const broad = {...light, x: 0, y: 0, z: -4, range: 200};
  const frame = owner.update([broad, broad, broad], camera);
  assert.equal(frame.pointLightCount, 2);
  assert.equal(frame.pointLights.byteLength, 2*32);
  assert.ok(frame.clusterLightCounts.data.every(count => count === 1));
  assert.ok(frame.clusterLightIndices.data.every(index => index === 0));
  assert.deepEqual(owner.stats, {sourceLightCount: 3, pointLightCount: 2, droppedLightCount: 1,
    droppedClusterLightCount: 8, clusterCount: 8});
  const resized = owner.update([], {...camera, width: 100000, height: 100000});
  assert.equal(resized.clusterCountX, 16);
  assert.equal(resized.clusterCountY, 9);
  assert.equal(resized.clusterCountZ, 2);
  owner.destroy();
});

test('volume-light storage reuses capacity and cleans up replacements, failures and disposal', () => {
  const device = createDevice();
  const owner = new SceneVolumeLights(device, {clusterCountZ: 2});
  const smallCamera = {...camera, width: 128, height: 128};
  const small = owner.update([light], smallCamera);
  owner.update([light], smallCamera);
  assert.equal(device.buffers.length, 3);

  // Failed replacement releases its partial allocation while preserving active bindings.
  device.failAllocation = device.allocationCount + 2;
  assert.throws(() => owner.update([light], camera), /Allocation failed/);
  assert.equal(device.buffers.at(-1).destroyCount, 1);
  assert.equal(small.clusterLightCounts.destroyCount, 0);
  assert.equal(small.clusterLightIndices.destroyCount, 0);
  const recovered = owner.update([light], smallCamera);
  assert.equal(recovered.clusterLightCounts, small.clusterLightCounts);

  const grown = owner.update([light], camera);
  assert.equal(grown.pointLights, small.pointLights);
  assert.equal(small.clusterLightCounts.destroyCount, 1);
  assert.equal(small.clusterLightIndices.destroyCount, 1);
  const shrunk = owner.update([], smallCamera);
  assert.equal(shrunk.clusterLightCounts, grown.clusterLightCounts);
  assert.equal(shrunk.clusterLightIndices, grown.clusterLightIndices);
  owner.destroy();
  owner.destroy();
  assert.ok(device.buffers.every(buffer => buffer.destroyCount === 1));
  assert.equal(device.destroyCount, 0);
  assert.throws(() => owner.update([], camera), /destroyed/);

  const failedDevice = createDevice();
  failedDevice.failAllocation = 3;
  const failedOwner = new SceneVolumeLights(failedDevice);
  assert.throws(() => failedOwner.update([light], camera), /Allocation failed/);
  assert.ok(failedDevice.buffers.every(buffer => buffer.destroyCount === 1));
  failedOwner.destroy();
});

test('invalid volume-light inputs fail before GPU allocation or writes', () => {
  const device = createDevice();
  assert.throws(() => new SceneVolumeLights({...device, type: 'webgl'}), /WebGPU/);
  assert.throws(() => new SceneVolumeLights(device, {maxPointLights: 33}), /32/);
  assert.throws(() => new SceneVolumeLights(device, {maxLightsPerCluster: 9}), /8/);
  const owner = new SceneVolumeLights(device);
  assert.throws(() => owner.update([light], {...camera, nearPlane: 0}), /clipping planes/);
  assert.throws(() => owner.update([light], {...camera, projectionMatrix: Array(16).fill(0)}), /nondegenerate/);
  for(const change of [{range: 0}, {range: 1e-100}, {x: 1e100}, {r: -1}]) {
    assert.throws(() => owner.update([{...light, ...change}], camera));
  }
  assert.equal(device.buffers.length, 0);
  device.limits.maxStorageBufferBindingSize = 32;
  assert.throws(() => owner.update([light], camera), /binding limits/);
  assert.equal(device.buffers.length, 0);
  owner.destroy();
});
