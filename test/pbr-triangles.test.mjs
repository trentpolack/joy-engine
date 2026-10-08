// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { PbrTriangles } from '../src/rendering/pbr/pbr-triangles.ts';
import { GpuTriangleRenderer } from 'joy-engine';

const TEST_DEVICE_INFO = Object.freeze({
  type: 'webgpu',
  vendor: 'test',
  renderer: 'unit-test-device',
  version: '1',
  gpu: 'software',
  gpuType: 'cpu',
  shadingLanguage: 'wgsl',
  shadingLanguageVersion: 100
});

function gpu() {
  const allocations = [];
  let fail = '';
  function allocate(properties) {
    if(properties.id === fail) {
      throw new Error('GPU allocation failed');
    }
    const resource = {
      properties, destroyed: 0, writes: [], shaderLayout: {}, bufferLayout: [],
      destroy() { this.destroyed++; },
      write(data) { this.writes.push(Array.from(data)); },
      setIndexBuffer(buffer) { this.indices = buffer; },
      setBuffer(_index, buffer) { this.vertices = buffer; }
    };
    allocations.push(resource);
    return resource;
  }
  return {
    allocations, fail(id) { fail = id; },
    device: {
      type: 'webgpu', info: TEST_DEVICE_INFO, features: new Set(),
      limits: { maxTextureDimension2D: 2048, maxStorageBuffersInVertexStage: 0 },
      createTexture: allocate, createBuffer: allocate, createShader: allocate,
      createRenderPipeline: allocate, createVertexArray: allocate
    }
  };
}

function mesh(source = null, alphaMode = 'OPAQUE') {
  return {
    positions: [0, 0, 0, 1, 0, 0, 0, 1, 0], normals: Array(9).fill(0),
    colors: Array(9).fill(1), alphas: [1, 1, 1], uvs: [0, 0, 1, 0, 0, 1],
    triangles: [0, 1, 2], triangleMaterials: [0],
    materials: [{
      baseColor: [1, 1, 1, 1], emissive: [0, 0, 0], metallic: 0, roughness: 0.5,
      alphaMode, alphaCutoff: 0.5,
      textures: { baseColor: source, normal: null, emissive: null, metallic: null, roughness: null, ao: null }
    }]
  };
}

test('PBR scene releases partial startup resources when pipeline creation fails', () => {
  const renderer = gpu();
  renderer.fail('pbr-transparent');
  assert.throws(() => new PbrTriangles(renderer.device), /GPU allocation failed/);
  assert.ok(renderer.allocations.length > 0);
  assert.ok(renderer.allocations.every(resource => resource.destroyed === 1));
});

test('PBR mesh allocation failure preserves active draw resources and frees pending buffers', async t => {
  const renderer = gpu();
  const scene = new PbrTriangles(renderer.device);
  t.after(() => scene.destroy());
  await scene.setMesh(mesh());
  const activeVertices = scene.vertices;
  const activeMaterials = scene.materials.bindings;
  const previousAllocations = renderer.allocations.length;
  renderer.fail('pbr-indices');
  await assert.rejects(scene.setMesh(mesh()), /GPU allocation failed/);
  assert.equal(scene.vertices, activeVertices);
  assert.equal(scene.materials.bindings, activeMaterials);
  assert.equal(activeVertices.destroyed, 0);
  assert.ok(renderer.allocations.slice(previousAllocations).every(resource => resource.destroyed === 1));
  scene.destroy();
  assert.equal(await scene.setMesh(mesh()), false);
  assert.ok(renderer.allocations.every(resource => resource.destroyed === 1));
});

test('PBR mesh and materials stay paired when a synchronous update is superseded by a failed image load', async t => {
  const renderer = gpu();
  const scene = new PbrTriangles(renderer.device);
  t.after(() => scene.destroy());
  await scene.setMesh(mesh());
  t.mock.method(globalThis, 'fetch', async () => { throw new Error('Image load failed'); });
  const replacing = scene.setMesh(mesh(null, 'BLEND'));
  const failed = scene.setMesh(mesh('broken.png'));
  await replacing;
  await assert.rejects(failed, /Image load failed/);
  // The committed material mode and CPU draw classification must always agree, and the
  // accepted (BLEND) upload stays active after the newer image load fails.
  const activeMode = scene.materials.bindings[0].uniforms.properties.data[9];
  assert.equal(scene.definitions[0].alphaMode, activeMode === 2 ? 'BLEND' : 'OPAQUE');
  assert.equal(scene.definitions[0].alphaMode, 'BLEND');
  assert.equal(scene.vertices.destroyed, 0);
});

test('destroying a PBR scene during image loading releases active and pending mesh allocations once', async t => {
  const renderer = gpu();
  const scene = new PbrTriangles(renderer.device);
  t.after(() => scene.destroy());
  await scene.setMesh(mesh());
  let rejectImage;
  t.mock.method(globalThis, 'fetch', () => new Promise((_resolve, reject) => { rejectImage = reject; }));
  const pending = scene.setMesh(mesh('pending.png'));
  scene.destroy();
  rejectImage(new Error('cancelled'));
  assert.equal(await pending, false);
  scene.destroy();
  assert.ok(renderer.allocations.every(resource => resource.destroyed === 1));
});

test('PBR transparent triangles are sorted back-to-front without depth writes', async t => {
  const renderer = gpu();
  const scene = new PbrTriangles(renderer.device);
  t.after(() => scene.destroy());
  const data = mesh(null, 'BLEND');
  data.positions.push(0, 0, -4, 1, 0, -4, 0, 1, -4);
  data.normals.push(...Array(9).fill(0));
  data.colors.push(...Array(9).fill(1));
  data.alphas.push(1, 1, 1);
  data.uvs.push(0, 0, 1, 0, 0, 1);
  data.triangles.push(3, 4, 5);
  data.triangleMaterials.push(0);
  await scene.setMesh(data);
  scene.prepare({ viewProjection: Array(16).fill(0), eye: [0, 0, 8], backward: [0, 0, 1] });
  assert.deepEqual(scene.indices.writes.at(-1), [3, 4, 5, 0, 1, 2]);
  assert.equal(scene.transparentPipeline.properties.parameters.depthWriteEnabled, false);
  const draws = [];
  const pass = { setPipeline() {}, setVertexArray() {}, setBindings() {}, draw(value) { draws.push(value); } };
  scene.draw(pass, false);
  assert.equal(draws.length, 0);
  scene.draw(pass, true);
  assert.deepEqual(draws, [{ firstIndex: 0, indexCount: 6 }]);
  // WebGL's installed adapter consumes byte offsets through firstVertex;
  // WebGPU consumes index units through firstIndex.
  draws.length = 0;
  scene.device.type = 'webgl';
  scene.drawRange(pass, {material: 0, firstIndex: 3, indexCount: 3}, true);
  assert.deepEqual(draws, [{firstVertex: 12, indexCount: 3}]);
});

test('mixed PBR, colored and textured transparency shares depth order and coalesces adjacent ranges', async t => {
  const gpuResources = gpu();
  const scene = new PbrTriangles(gpuResources.device);
  t.after(() => scene.destroy());
  const data = mesh(null, 'BLEND');
  data.positions = [0, 0, -3, 1, 0, -3, 0, 1, -3, 0, 0, -2, 1, 0, -2, 0, 1, -2];
  data.normals.push(...Array(9).fill(0));
  data.colors.push(...Array(9).fill(1));
  data.alphas.push(1, 1, 1);
  data.uvs.push(0, 0, 1, 0, 0, 1);
  data.triangles.push(3, 4, 5);
  data.triangleMaterials.push(0);
  await scene.setMesh(data);
  const draws = [];
  let pipeline;
  const pass = {
    setPipeline(value) { pipeline = value; }, setVertexArray() {}, setBindings() {}, end() {},
    draw(range) {
      if(range.vertexCount || range.indexCount) {
        draws.push({ kind: pipeline === scene.transparentPipeline ? 'pbr' : 'colored', ...range });
      }
    }
  };
  const renderer = Object.assign(Object.create(GpuTriangleRenderer.prototype), {
    pbr: scene, vertexBuffer: { write() {} }, uniformBuffer: { write() {} }, vertexArray: {},
    pipeline: {}, transparentPipeline: {}, sceneUniformBufferSize: 64,
    canvasContext: { getCurrentFramebuffer() { return {}; } },
    device: { beginRenderPass() { return pass; }, submit() {} },
    textured: { prepare() {}, draw(_pass, _uniforms, _texture, firstVertex, vertexCount) {
      draws.push({ kind: 'textured', firstVertex, vertexCount });
    } }
  });
  const triangle = depth => [0, 0, depth, 1, 1, 1, 0.5, 1, 0, depth, 1, 1, 1, 0.5, 0, 1, depth, 1, 1, 1, 0.5];
  const colored = [...triangle(-4), ...triangle(-2.5), ...triangle(-0.5), ...triangle(-0.25)];
  const textured = [0, 0, -1, 1, 1, 1, 0.5, 0, 0, 1, 0, -1, 1, 1, 1, 0.5, 1, 0, 0, 1, -1, 1, 1, 1, 0.5, 0, 1];
  const frame = { viewProjection: Array(16).fill(0), eye: [0, 0, 8], backward: [0, 0, 1] };
  renderer.render([], new Float32Array(16), [0, 0, 0, 1], colored, 0, [{ vertices: textured, texture: {} }], frame);
  assert.deepEqual(draws, [
    { kind: 'colored', firstVertex: 0, vertexCount: 3 },
    { kind: 'pbr', firstIndex: 0, indexCount: 3 },
    { kind: 'colored', firstVertex: 3, vertexCount: 3 },
    { kind: 'pbr', firstIndex: 3, indexCount: 3 },
    { kind: 'textured', firstVertex: 0, vertexCount: 3 },
    { kind: 'colored', firstVertex: 6, vertexCount: 6 }
  ]);
});

test('failed renderer construction releases its canvas and device', async t => {
  const { luma } = await import('@luma.gl/core');
  const canvasContext = { destroyed: 0, destroy() { this.destroyed++; } };
  const device = {
    type: 'webgpu', info: TEST_DEVICE_INFO, features: new Set(),
    limits: { maxStorageBuffersInVertexStage: 0 }, canvasContext,
    destroyed: 0, destroy() { this.destroyed++; },
    createShader() { throw new Error('shader allocation failed'); }
  };
  t.mock.method(luma, 'createDevice', async () => device);
  await assert.rejects(GpuTriangleRenderer.create({}, { shaderSources: { wgsl: '', glsl: { vertex: '', fragment: '' } } }), /shader allocation failed/);
  assert.equal(canvasContext.destroyed, 1);
  assert.equal(device.destroyed, 1);
});
