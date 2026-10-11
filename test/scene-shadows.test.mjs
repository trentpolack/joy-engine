// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import {SceneShadows} from '../src/rendering/shadows/scene-shadows.ts';
import {UniformStore} from '@luma.gl/core';
import {OrbitCamera} from '../src/rendering/camera/orbit-camera.ts';

function gpu() {
  const allocations = [];
  function allocate(props) {
    const value = {props, ...props, destroyed: 0, writes: [], destroy() { this.destroyed++; },
      write(data) { this.writes.push(Array.from(data)); }, createView: allocate};
    allocations.push(value);
    return value;
  }
  return {allocations, device: {type: 'webgpu', limits: {maxTextureDimension2D: 4096},
    createTexture: allocate, createSampler: allocate, createFramebuffer: allocate, createBuffer: allocate,
    beginRenderPass: () => ({end() {}})}};
}

test('shadow owners cover more than four lights and release resources once', () => {
  const {device, allocations} = gpu();
  const owner = new SceneShadows(device);
  const orbit = new OrbitCamera();
  const camera = {viewMatrix: orbit.viewMatrix(), projectionMatrix: orbit.projectionMatrix(1, true), nearPlane: 0.1, farPlane: 100};
  const lights = Array.from({length: 5}, (_, x) => ({x, y: 3, z: 0, r: 1, g: 1, b: 1, range: 10}));
  const matrices = [];
  const batches = owner.prepare(camera, lights, null, view => matrices.push(view.viewProjectionMatrix));
  assert.equal(batches.length, 2);
  assert.equal(matrices.length, 30);
  assert.equal(owner.stats.castingPointLights, 5);
  assert.notEqual(batches[0].bindings.shadow, batches[1].bindings.shadow);
  lights[0].castsShadow = false;
  owner.prepare(camera, lights, null, () => {});
  assert.equal(owner.stats.castingPointLights, 4);
  owner.destroy();
  owner.destroy();
  assert.ok(allocations.every(resource => resource.destroyed === 1));
});

test('WebGL shadow incompatibility is explicit before allocation', () => {
  assert.throws(() => new SceneShadows({type: 'webgl'}), /WebGPU/);
});


test('shadow uniform startup failure releases all completed owner allocations', t => {
  const {device, allocations} = gpu();
  const owner = new SceneShadows(device);
  const orbit = new OrbitCamera();
  t.mock.method(UniformStore.prototype, 'getManagedUniformBuffer', () => { throw new Error('uniform startup failed'); });
  assert.throws(() => owner.prepare({viewMatrix: orbit.viewMatrix(), projectionMatrix: orbit.projectionMatrix(1, true),
    nearPlane: 0.1, farPlane: 100}, [], null, () => {}), /uniform startup failed/);
  owner.destroy();
  assert.ok(allocations.every(resource => resource.destroyed === 1));
});

test('shadow view clip depth includes casters at the point-light near plane', () => {
  const {device} = gpu();
  const owner = new SceneShadows(device);
  const orbit = new OrbitCamera();
  let nearDepth;
  let farDepth;
  owner.prepare({viewMatrix: orbit.viewMatrix(), projectionMatrix: orbit.projectionMatrix(1, true), nearPlane: 0.1, farPlane: 100},
    [{x:0,y:0,z:0,r:1,g:1,b:1,range:10}], null, view => {
      if(view.pointFace === '+X') {
        const matrix = view.viewProjectionMatrix;
        const near = view.camera.near;
        nearDepth = (matrix[2]*near + matrix[14])/(matrix[3]*near + matrix[15]);
        const far = view.camera.far;
        farDepth = (matrix[2]*far + matrix[14])/(matrix[3]*far + matrix[15]);
      }
    });
  owner.destroy();
  assert.ok(Math.abs(nearDepth) < 0.0001, `WebGPU near depth must be zero, received ${nearDepth}`);
  assert.ok(Math.abs(farDepth - 1) < 0.0001, `WebGPU far depth must be one, received ${farDepth}`);
});
