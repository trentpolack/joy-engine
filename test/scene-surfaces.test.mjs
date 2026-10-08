// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import { SceneSurfaces } from '../src/rendering/scene/scene-surfaces.ts';

const identity = new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
const triangle = [0, 0, -1, 1, 1, 1, 1, 1, 0, -1, 1, 1, 1, 1, 0, 1, -1, 1, 1, 1, 1];

test('surface attachment preserves opaque coverage, authored roughness and borrowed depth lifetime', () => {
  const device = createDevice();
  const depth = resource({width: 32, height: 24, format: 'depth24plus'});
  const surfaces = new SceneSurfaces(device);
  const normalTexture = surfaces.render(triangle, identity, identity, depth, [0.2, 0.2, 0.2]);
  const framebuffer = device.resources.find(value => value.kind === 'framebuffer');
  const pipeline = device.resources.find(value => value.kind === 'pipeline');
  const buffer = device.resources.find(value => value.kind === 'buffer' && value.id === 'scene-surface-vertices');

  assert.equal(normalTexture.format, 'rgba16float');
  assert.equal(framebuffer.depthStencilAttachment, depth);
  assert.deepEqual(pipeline.parameters, {depthWriteEnabled: false, depthCompare: 'equal', blend: false});
  assert.equal(device.passes[0].clearDepth, false);
  assert.equal(device.passes[0].depthReadOnly, true);
  assert.deepEqual(device.passes[0].draws, [{vertexCount: 3}]);
  assert.equal(device.passes[0].ended, true);
  assert.deepEqual(Array.from(buffer.data), Array.from(new Float32Array([0, 0, -1, 0.2, 1, 0, -1, 0.2, 0, 1, -1, 0.2])));

  assert.equal(surfaces.render(triangle, identity, identity, depth), normalTexture);
  assert.equal(buffer.data[3], 1);
  assert.equal(buffer.data[7], 1);
  assert.equal(buffer.data[11], 1);

  const replacementDepth = resource({width: 64, height: 48, format: 'depth24plus'});
  const replacement = surfaces.render([], identity, identity, replacementDepth);
  assert.notEqual(replacement, normalTexture);
  assert.equal(normalTexture.destroyCount, 1);
  assert.equal(framebuffer.destroyCount, 1);
  assert.deepEqual(device.passes[2].draws, []);
  surfaces.destroy();
  surfaces.destroy();
  assert.equal(depth.destroyCount, 0);
  assert.equal(replacementDepth.destroyCount, 0);
  assert.equal(device.destroyCount, 0);
  assert.ok(device.resources.every(value => value.destroyCount === 1));
});

test('surface resize rolls back incomplete attachments and rebinds replacement depth at the same size', () => {
  const device = createDevice();
  const surfaces = new SceneSurfaces(device);
  const depth = resource({width: 32, height: 24, format: 'depth24plus'});
  const original = surfaces.render(triangle, identity, identity, depth);
  const replacementDepth = resource({width: 32, height: 24, format: 'depth24plus'});
  device.failFramebuffer = true;
  assert.throws(() => surfaces.resize(replacementDepth), /allocation failure/);
  assert.equal(original.destroyCount, 0);
  assert.equal(device.resources.at(-1).destroyCount, 1);
  device.failFramebuffer = false;
  assert.equal(surfaces.render(triangle, identity, identity, depth), original);
  const replacement = surfaces.render(triangle, identity, identity, replacementDepth);
  assert.notEqual(replacement, original);
  assert.equal(original.destroyCount, 1);
  surfaces.destroy();
  assert.equal(depth.destroyCount, 0);
  assert.equal(replacementDepth.destroyCount, 0);
  assert.ok(device.resources.every(value => value.destroyCount === 1));
});

test('surface input validation and partial construction never leak resources or submit invalid draws', () => {
  const device = createDevice();
  const depth = resource({width: 32, height: 24, format: 'depth24plus'});
  const surfaces = new SceneSurfaces(device);
  assert.throws(() => surfaces.render(triangle, identity, identity, depth, [0.5]), /roughness/);
  assert.throws(() => surfaces.render(triangle, identity, identity, depth, [NaN, 0, 0]), /roughness/);
  assert.throws(() => surfaces.render(triangle, new Float32Array(4), identity, depth), /matrices/);
  assert.throws(() => surfaces.render(triangle.slice(1), identity, identity, depth), /triangles/);
  assert.equal(device.passes.length, 0);
  surfaces.destroy();
  assert.throws(() => surfaces.render(triangle, identity, identity, depth), /destroyed/);

  const failing = createDevice();
  failing.createVertexArray = () => { throw new Error('allocation failure'); };
  assert.throws(() => new SceneSurfaces(failing), /allocation failure/);
  assert.ok(failing.resources.every(value => value.destroyCount === 1));
  assert.throws(() => new SceneSurfaces({type: 'webgl'}), /WebGPU/);
});

function resource(properties) {
  return {...properties, destroyCount: 0, destroy() { this.destroyCount++; }};
}

function createDevice() {
  const device = {
    type: 'webgpu', resources: [], passes: [], destroyCount: 0, failFramebuffer: false,
    create(kind, properties) {
      const created = resource({kind, ...properties});
      this.resources.push(created);
      return created;
    },
    createShader(properties) { return this.create('shader', properties); },
    createRenderPipeline(properties) { return this.create('pipeline', {...properties, shaderLayout: {}}); },
    createVertexArray(properties) { return this.create('vertexArray', {...properties, setBuffer() {}}); },
    createBuffer(properties) {
      return this.create('buffer', {...properties, write(data) { this.data = data.slice(); }});
    },
    createTexture(properties) { return this.create('texture', properties); },
    createFramebuffer(properties) {
      if(this.failFramebuffer) {
        throw new Error('allocation failure');
      }
      return this.create('framebuffer', properties);
    },
    beginRenderPass(properties) {
      const pass = {
        ...properties, draws: [], ended: false,
        setPipeline() {}, setBindings() {}, setVertexArray() {},
        draw(options) { this.draws.push(options); },
        end() { this.ended = true; }
      };
      this.passes.push(pass);
      return pass;
    }
  };
  return device;
}
