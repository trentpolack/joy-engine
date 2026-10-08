// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import * as engine from 'joy-engine';

/** Build a minimal in-memory GLB fixture with interleaved vertex attributes and 32-bit indices.
 * @param {string[]} [meshNames]
 */
function createTriangleGlb(meshNames = ['triangle']) {
  const json = {
    asset: { version: '2.0' },
    buffers: [{ byteLength: 84 }],
    meshes: meshNames.map((name) => ({
      name,
      primitives: [{ attributes: { POSITION: 0, NORMAL: 1 }, indices: 2 }],
    })),
    accessors: [
      { bufferView: 0, byteOffset: 0, componentType: 5126, type: 'VEC3', count: 3 },
      { bufferView: 0, byteOffset: 12, componentType: 5126, type: 'VEC3', count: 3 },
      { bufferView: 1, componentType: 5125, type: 'SCALAR', count: 3 },
    ],
    bufferViews: [
      { buffer: 0, byteOffset: 0, byteLength: 72, byteStride: 24 },
      { buffer: 0, byteOffset: 72, byteLength: 12 },
    ],
  };
  const text = JSON.stringify(json);
  const encoded = new TextEncoder().encode(text.padEnd(Math.ceil(text.length / 4) * 4));
  const buffer = new ArrayBuffer(12 + 8 + encoded.length + 8 + 84);
  const view = new DataView(buffer);
  [0x46546c67, 2, buffer.byteLength, encoded.length, 0x4e4f534a].forEach(
    (value, index) => view.setUint32(index * 4, value, true),
  );
  new Uint8Array(buffer, 20, encoded.length).set(encoded);
  const binaryHeader = 20 + encoded.length;
  view.setUint32(binaryHeader, 84, true);
  view.setUint32(binaryHeader + 4, 0x004e4942, true);
  new Float32Array(buffer, binaryHeader + 8, 18).set([
    0, 0, 0, 0, 1, 0,
    2, 0, 0, 0, 1, 0,
    0, 0, 4, 0, 1, 0,
  ]);
  new Uint32Array(buffer, binaryHeader + 80, 3).set([0, 1, 2]);
  return buffer;
}

/**
 * Build a JSON glTF data URL whose geometry resides in a linked data URI.
 * @param {{positions?: number[], normals?: number[], indices?: number[], meshCount?: number, primitiveCount?: number}} [options]
 */
function createTriangleGltfUrl(options = {}) {
  const positions = options.positions ?? [0, 0, 0, 2, 0, 0, 0, 0, 4];
  const normals = options.normals ?? [0, 1, 0, 0, 1, 0, 0, 1, 0];
  const indices = options.indices ?? [0, 1, 2];
  const positionsByteLength = positions.length*4;
  const normalsByteOffset = positionsByteLength;
  const normalsByteLength = normals.length*4;
  const indicesByteOffset = normalsByteOffset + normalsByteLength;
  const indicesByteLength = indices.length*2;
  const binaryByteLength = Math.ceil((indicesByteOffset + indicesByteLength)/4)*4;
  const binary = Buffer.alloc(binaryByteLength);
  new Float32Array(binary.buffer, binary.byteOffset, positions.length).set(positions);
  new Float32Array(binary.buffer, binary.byteOffset + normalsByteOffset, normals.length).set(normals);
  new Uint16Array(binary.buffer, binary.byteOffset + indicesByteOffset, indices.length).set(indices);
  const primitive = { attributes: { POSITION: 0, NORMAL: 1 }, indices: 2 };
  const json = {
    asset: { version: '2.0' },
    buffers: [{ byteLength: binaryByteLength, uri: `data:application/octet-stream;base64,${binary.toString('base64')}` }],
    bufferViews: [
      { buffer: 0, byteOffset: 0, byteLength: positionsByteLength },
      { buffer: 0, byteOffset: normalsByteOffset, byteLength: normalsByteLength },
      { buffer: 0, byteOffset: indicesByteOffset, byteLength: indicesByteLength },
    ],
    accessors: [
      { bufferView: 0, componentType: 5126, type: 'VEC3', count: positions.length/3 },
      { bufferView: 1, componentType: 5126, type: 'VEC3', count: normals.length/3 },
      { bufferView: 2, componentType: 5123, type: 'SCALAR', count: indices.length },
    ],
    meshes: Array.from({ length: options.meshCount ?? 1 }, (_, meshIndex) => ({
      name: meshIndex === 0 ? 'linked-triangle' : `extra-${meshIndex}`,
      primitives: Array.from({ length: options.primitiveCount ?? 1 }, () => primitive)
    }))
  };
  return `data:model/gltf+json;base64,${Buffer.from(JSON.stringify(json)).toString('base64')}`;
}

test('GLB decoding preserves interleaved model-space geometry and 32-bit indices', async () => {
  assert.equal(typeof engine.parseGlbMesh, 'function');
  assert.deepEqual(await engine.parseGlbMesh(createTriangleGlb()), {
    name: 'triangle',
    positions: [0, 0, 0, 2, 0, 0, 0, 0, 4],
    normals: [0, 1, 0, 0, 1, 0, 0, 1, 0],
    indices: [0, 1, 2],
  });
});

test('GLB decoding selects a named mesh from a multi-object scene export', async () => {
  const buffer = createTriangleGlb(['unrelated', 'requested']);

  assert.equal((await engine.parseGlbMesh(buffer, 'requested')).name, 'requested');
  await assert.rejects(engine.parseGlbMesh(buffer, 'missing'), /requested mesh "missing"/);
});

test('glTF loading resolves geometry stored in a linked buffer URI', async () => {
  assert.deepEqual(await engine.loadGlbMesh(createTriangleGltfUrl()), {
    name: 'linked-triangle',
    positions: [0, 0, 0, 2, 0, 0, 0, 0, 4],
    normals: [0, 1, 0, 0, 1, 0, 0, 1, 0],
    indices: [0, 1, 2],
  });
});

test('glTF loading rejects malformed indexed geometry', async () => {
  await assert.rejects(engine.loadGlbMesh(createTriangleGltfUrl({
    normals: [0, 1, 0, 0, 1, 0]
  })), /matching position and normal counts/);
  await assert.rejects(engine.loadGlbMesh(createTriangleGltfUrl({
    indices: [0, 1]
  })), /complete triangles/);
  await assert.rejects(engine.loadGlbMesh(createTriangleGltfUrl({
    indices: [0, 1, 3]
  })), /outside the vertex range/);
  await assert.rejects(engine.loadGlbMesh(createTriangleGltfUrl({
    positions: [], normals: [], indices: []
  })), /at least one indexed triangle/);
  await assert.rejects(engine.loadGlbMesh(createTriangleGltfUrl({
    positions: [Number.NaN, 0, 0, 2, 0, 0, 0, 0, 4]
  })), /finite position and normal values/);
});

test('glTF loading rejects ambiguous or missing meshes', async () => {
  await assert.rejects(engine.loadGlbMesh(createTriangleGltfUrl({
    primitiveCount: 2
  })), /exactly one primitive/);
  await assert.rejects(engine.loadGlbMesh(createTriangleGltfUrl({
    meshCount: 0
  })), /does not contain a mesh/);
});

test('mesh batching transforms geometry and shades RGB while preserving alpha and source data', () => {
  assert.equal(typeof engine.addMesh, 'function');
  const mesh = Object.freeze({ name: 'triangle', triangles: Object.freeze([
    Object.freeze([0, 0, 0, 1, 0, 0, 0, 1, 0, 2]),
  ]) });
  const vertices = [];
  engine.addMesh(vertices, mesh, 10, 20, 2, Math.PI / 2, [0.75, 0.25, 0, 0.4]);
  assert.deepEqual(vertices, [
    10, 20, 0, 1, 0.5, 0, 0.4,
    10, 22, 0, 1, 0.5, 0, 0.4,
    8, 20, 0, 1, 0.5, 0, 0.4,
  ]);
});
