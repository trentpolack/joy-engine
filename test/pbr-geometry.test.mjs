// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import { preparePbrGeometry } from '../src/rendering/geometry/pbr-geometry.ts';

test('PBR geometry preserves hard edges, authored normals and per-corner data without mutating source', () => {
  const mesh = {positions:[0,0,0, 1,0,0, 0,1,0, 0,0,1], normals:Array(12).fill(0), colors:Array(12).fill(1), alphas:[1,0.5,1,1], uvs:[0,0,1,0,0,1,1,1], triangles:[0,1,2,0,3,1], triangleMaterials:[0,0], materials:[{}]};
  const original = structuredClone(mesh);
  let prepared = preparePbrGeometry(mesh);
  assert.deepEqual([...prepared.vertices.slice(3,6)], [0,0,1]);
  assert.deepEqual([...prepared.vertices.slice(39,42)], [0,1,0]);
  assert.equal(prepared.vertices[21], 0.5);
  assert.deepEqual(mesh, original);
  mesh.normals.splice(0,3, 2,0,0);
  prepared = preparePbrGeometry(mesh);
  assert.deepEqual([...prepared.vertices.slice(3,6)], [1,0,0]);
  assert.deepEqual([...prepared.vertices.slice(39,42)], [1,0,0]);
  mesh.normals[0] = Number.MAX_VALUE;
  assert.throws(() => preparePbrGeometry(mesh), /float32/);
  mesh.normals[0] = 1;
  mesh.positions[0] = 1e40;
  assert.throws(() => preparePbrGeometry(mesh), /float32/);
  mesh.positions[0] = 0;
  mesh.triangles[0] = 9;
  assert.throws(() => preparePbrGeometry(mesh), /outside/);
});
