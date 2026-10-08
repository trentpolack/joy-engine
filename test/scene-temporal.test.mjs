// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import { prepareSceneTemporalFrame } from '../src/rendering/scene/scene-temporal.ts';
import { SceneMotionPass } from '../src/rendering/scene/scene-motion.ts';

const matrix = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
const triangle = x => [x, 0, 0, 1, 1, 1, 1, x + 1, 0, 0, 1, 1, 1, 1, x, 1, 0, 1, 1, 1, 1];
const xyz = x => [x, 0, 0, x + 1, 0, 0, x, 1, 0];
const range = (identity, topology, firstVertex = 0) => ({identity, topology, firstVertex, vertexCount: 3});

test('object identities preserve previous world positions through movement and queue reorder', () => {
  const scene = {};
  const a = {};
  const b = {};
  const asset = {};
  const frame = {scene, vertices: [...triangle(0), ...triangle(10)], ranges: [range(a, asset), range(b, asset, 3)], viewProjection: [...matrix]};
  const first = prepareSceneTemporalFrame(null, frame);
  assert.equal(first.resetHistory, true);
  assert.deepEqual([...first.previousPositions], [...xyz(0), ...xyz(10)]);
  frame.vertices[0] = 999;
  frame.viewProjection[12] = 999;
  const movedCamera = [...matrix];
  movedCamera[12] = 0.5;
  const second = prepareSceneTemporalFrame(first.snapshot, {
    scene, vertices: [...triangle(12), ...triangle(3)], ranges: [range(b, asset), range(a, asset, 3)], viewProjection: movedCamera
  });
  assert.equal(second.resetHistory, false);
  assert.deepEqual([...second.previousPositions], [...xyz(10), ...xyz(0)]);
  assert.deepEqual([...second.previousViewProjection], matrix, 'snapshot copies the previous camera');
  assert.deepEqual([...second.snapshot.meshes.get(a).positions], xyz(3));
});

test('replacement, reappearance, scene changes and explicit cuts never reuse unrelated object history', () => {
  const scene = {};
  const identity = {};
  const topology = {};
  const input = {scene, vertices: triangle(0), ranges: [range(identity, topology)], viewProjection: matrix};
  const first = prepareSceneTemporalFrame(null, input);
  const replaced = prepareSceneTemporalFrame(first.snapshot, {...input, vertices: triangle(20), ranges: [range(identity, {})]});
  assert.equal(replaced.resetHistory, true);
  assert.deepEqual([...replaced.previousPositions], xyz(20));
  const hidden = prepareSceneTemporalFrame(first.snapshot, {...input, vertices: [], ranges: []});
  assert.equal(hidden.resetHistory, true);
  assert.equal(hidden.snapshot.meshes.size, 0);
  const visible = prepareSceneTemporalFrame(hidden.snapshot, {...input, vertices: triangle(30)});
  assert.equal(visible.resetHistory, true);
  assert.deepEqual([...visible.previousPositions], xyz(30));
  for(const change of [{scene: {}}, {resetHistory: true}]) {
    const newCamera = [...matrix];
    newCamera[12] = 5;
    const reset = prepareSceneTemporalFrame(first.snapshot, {...input, ...change, vertices: triangle(40), viewProjection: newCamera});
    assert.equal(reset.resetHistory, true);
    assert.deepEqual([...reset.previousPositions], xyz(40));
    assert.deepEqual([...reset.previousViewProjection], newCamera);
  }
});

test('unidentified triangles invalidate history and invalid correspondence fails without mutating the committed frame', () => {
  const identity = {};
  const topology = {};
  const input = {scene: {}, vertices: triangle(0), ranges: [range(identity, topology)], viewProjection: matrix};
  const first = prepareSceneTemporalFrame(null, input);
  const unknown = prepareSceneTemporalFrame(first.snapshot, {...input, ranges: [], vertices: triangle(15)});
  assert.equal(unknown.resetHistory, true);
  assert.deepEqual([...unknown.previousPositions], xyz(15));
  assert.throws(() => prepareSceneTemporalFrame(first.snapshot, {...input, ranges: [range(identity, topology), range({}, topology)]}), /overlap/);
  assert.throws(() => prepareSceneTemporalFrame(first.snapshot, {...input, ranges: [range(identity, topology, 3)]}), /triangle runs/);
  assert.throws(() => prepareSceneTemporalFrame(first.snapshot, {...input, vertices: [1, 2]}), /complete/);
  const retry = prepareSceneTemporalFrame(first.snapshot, {...input, vertices: triangle(2)});
  assert.deepEqual([...retry.previousPositions], xyz(0));
});

test('motion pass owns both velocity attachments, preserves depth occlusion and releases replaced resources', () => {
  const gpu = fakeDevice();
  const motion = new SceneMotionPass(gpu.device);
  const frame = {vertices: triangle(2), previousPositions: new Float32Array(xyz(0)), currentViewProjection: matrix,
    previousViewProjection: matrix, velocityJitterDelta: [0.001, -0.002], width: 32, height: 24};
  const output = motion.render(frame);
  assert.equal(output.format, 'rgba16float');
  assert.notEqual(output, motion.jitteredVelocityTexture);
  assert.deepEqual(gpu.pipeline.parameters, {depthWriteEnabled: true, depthCompare: 'less-equal', blend: false});
  assert.deepEqual(gpu.passes[0].options.framebuffer.colorAttachments, [output, motion.jitteredVelocityTexture]);
  assert.equal(gpu.passes[0].options.clearDepth, 1);
  assert.equal(gpu.passes[0].drawn.vertexCount, 3);
  assert.equal(gpu.passes[0].ended, true);
  assert.deepEqual([...gpu.resources.find(resource => resource.id === 'scene-motion-previous-positions').data], xyz(0));
  assert.equal(motion.render(frame), output, 'same-sized frames retain attachments');
  const replacement = motion.render({...frame, width: 64});
  assert.notEqual(replacement, output);
  assert.equal(output.destroyCount, 1);
  assert.throws(() => motion.render({...frame, previousPositions: new Float32Array(3)}), /matching/);
  motion.destroy();
  motion.destroy();
  for(const resource of gpu.resources) {
    assert.equal(resource.destroyCount, 1, resource.id);
  }
  assert.equal(motion.jitteredVelocityTexture, null);
  assert.throws(() => motion.render(frame), /destroyed/);
});

test('partial motion target creation rolls back and preserves the previous usable output', () => {
  const gpu = fakeDevice();
  const motion = new SceneMotionPass(gpu.device);
  const frame = {vertices: [], previousPositions: new Float32Array(), currentViewProjection: matrix,
    previousViewProjection: matrix, width: 16, height: 16};
  const first = motion.render(frame);
  gpu.device.createFramebuffer = () => { throw new Error('allocation failed'); };
  assert.throws(() => motion.render({...frame, width: 32}), /allocation failed/);
  assert.equal(first.destroyCount, 0);
  assert.equal(motion.render(frame), first);
  motion.destroy();
  for(const resource of gpu.resources) {
    assert.equal(resource.destroyCount, 1, resource.id);
  }
});

function fakeDevice() {
  const resources = [];
  const passes = [];
  const state = {resources, passes, pipeline: null};
  function resource(options) {
    const value = {...options, destroyCount: 0, destroy() { this.destroyCount++; },
      write(data) { this.data = data.slice(); }, setBuffer() {}};
    resources.push(value);
    return value;
  }
  state.device = {
    type: 'webgpu',
    createShader: resource,
    createRenderPipeline(options) {
      state.pipeline = options;
      return resource({...options, shaderLayout: {}});
    },
    createVertexArray: resource,
    createBuffer: resource,
    createTexture: resource,
    createFramebuffer: resource,
    beginRenderPass(options) {
      const pass = {options, setPipeline() {}, setBindings() {}, setVertexArray() {},
        draw(drawn) { this.drawn = drawn; }, end() { this.ended = true; }};
      passes.push(pass);
      return pass;
    }
  };
  return state;
}
