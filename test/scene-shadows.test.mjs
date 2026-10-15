// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import { UniformStore } from '@luma.gl/core';
import { OrbitCamera } from '../src/rendering/camera/orbit-camera.ts';
import { createFrameView } from '../src/rendering/scene/frame-view.ts';
import { createRenderPassContext } from '../src/rendering/scene/render-pass-context.ts';
import { SceneShadows } from '../src/rendering/shadows/scene-shadows.ts';

/**
 * Create an isolated WebGPU device fixture that records resource allocation and cleanup.
 * @returns A borrowed-device stub and its mutable resource history for lifecycle assertions.
 */
function gpu() {
  const allocations = [];

  /**
   * Track a resource, its uploads, and destruction count without allocating GPU storage.
   * @param props Borrowed resource settings, copied onto the stub without mutation.
   * @returns A new resource stub retained in the fixture's allocation history.
   */
  function allocate(props) {
    const value = {
      props,
      ...props,
      destroyed: 0,
      writes: [],
      destroy() {
        this.destroyed++;
      },
      write(data) {
        this.writes.push(Array.from(data));
      },
      createView: allocate
    };
    allocations.push(value);
    return value;
  }

  return {
    allocations,
    device: {
      type: 'webgpu',
      limits: { maxTextureDimension2D: 4096 },
      createTexture: allocate,
      createSampler: allocate,
      createFramebuffer: allocate,
      createBuffer: allocate,
      beginRenderPass: props => ({
        props,
        end() {}
      })
    }
  };
}

test('shadow owners cover more than four lights and release resources once', () => {
  const { device, allocations } = gpu();
  const owner = new SceneShadows(device);
  const orbit = new OrbitCamera();
  const camera = {
    viewMatrix: orbit.viewMatrix(),
    projectionMatrix: orbit.projectionMatrix(1, true),
    nearPlane: 0.1,
    farPlane: 100
  };
  const lights = Array.from({ length: 5 }, (_, x) => ({
    x,
    y: 3,
    z: 0,
    r: 1,
    g: 1,
    b: 1,
    range: 10
  }));
  const view = createFrameView({
    camera: { ...camera, ...orbit.basis() },
    lights,
    viewport: {
      cssWidth: 800,
      cssHeight: 500,
      pixelRatio: 2
    }
  });
  const savedView = structuredClone(view);
  const matrices = [];
  const batches = owner.prepare(view.camera, view.lighting.lights, view.lighting.directional, context => {
    assert.ok(context.target, 'shadow targets belong to the pass context');
    assert.equal(context.viewport.width, context.target.width);
    assert.notEqual(context.camera.viewMatrix, view.camera.viewMatrix);
    assert.throws(() => {
      context.camera.viewMatrix[0] = 99;
    }, TypeError);
    matrices.push(context.camera.viewProjectionMatrix);
  });

  assert.equal(batches.length, 2);
  assert.equal(matrices.length, 30);
  assert.equal(owner.stats.castingPointLights, 5);
  assert.deepEqual(view, savedView, 'all light faces preserve the main view');

  const target = { width: 400, height: 250 };
  const context = createRenderPassContext({ end() {} }, target, view.camera);
  assert.equal(context.camera, view.camera, 'a target override does not rebuild the view camera');
  assert.deepEqual(context.viewport, { width: 400, height: 250 });
  assert.deepEqual(view.viewport, savedView.viewport);
  assert.equal(Object.isFrozen(target), false, 'GPU resource owners retain control of their targets');
  assert.notEqual(batches[0].bindings.shadow, batches[1].bindings.shadow);

  lights[0].castsShadow = false;
  owner.prepare(camera, lights, null, () => {});
  assert.equal(owner.stats.castingPointLights, 4);

  owner.destroy();
  owner.destroy();
  assert.ok(allocations.every(resource => resource.destroyed === 1));
});

test('WebGL shadow incompatibility is explicit before allocation', () => {
  assert.throws(() => new SceneShadows({ type: 'webgl' }), /WebGPU/);
});

test('shadow uniform startup failure releases all completed owner allocations', t => {
  const { device, allocations } = gpu();
  const owner = new SceneShadows(device);
  const orbit = new OrbitCamera();

  t.mock.method(UniformStore.prototype, 'getManagedUniformBuffer', () => {
    throw new Error('uniform startup failed');
  });
  assert.throws(() => owner.prepare({
    viewMatrix: orbit.viewMatrix(),
    projectionMatrix: orbit.projectionMatrix(1, true),
    nearPlane: 0.1,
    farPlane: 100
  }, [], null, () => {}), /uniform startup failed/);

  owner.destroy();
  assert.ok(allocations.every(resource => resource.destroyed === 1));
});

test('shadow view clip depth includes casters at the point-light near plane', () => {
  const { device } = gpu();
  const owner = new SceneShadows(device);
  const orbit = new OrbitCamera();
  let nearDepth;
  let farDepth;

  owner.prepare({
    viewMatrix: orbit.viewMatrix(),
    projectionMatrix: orbit.projectionMatrix(1, true),
    nearPlane: 0.1,
    farPlane: 100
  }, [{
    x: 0,
    y: 0,
    z: 0,
    r: 1,
    g: 1,
    b: 1,
    range: 10
  }], null, view => {
    if(view.pointFace === '+X') {
      const matrix = view.camera.viewProjectionMatrix;
      const near = view.camera.nearPlane;
      nearDepth = ((matrix[2]*near) + matrix[14])/((matrix[3]*near) + matrix[15]);
      const far = view.camera.farPlane;
      farDepth = ((matrix[2]*far) + matrix[14])/((matrix[3]*far) + matrix[15]);
    }
  });

  owner.destroy();
  assert.ok(Math.abs(nearDepth) < 0.0001, `WebGPU near depth must be zero, received ${nearDepth}`);
  assert.ok(Math.abs(farDepth - 1) < 0.0001, `WebGPU far depth must be one, received ${farDepth}`);
});
