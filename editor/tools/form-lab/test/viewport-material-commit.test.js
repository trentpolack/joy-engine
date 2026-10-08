// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { OrbitCamera } from 'joy-engine';
import { Viewport } from '../src/rendering/viewport.ts';

/**
 * Minimal triangle mesh; the source tags each request so the stub renderer can fail one.
 * @param {string | null} source
 * @param {string} alphaMode
 */
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

// The engine half of this scenario (PbrTriangles keeps the accepted upload when a newer
// image load fails) is covered in joy-engine/test/pbr-triangles.test.mjs. This case owns
// the editor half: CPU point/grid data must stay paired with the accepted mesh.
test('FORM LAB retains matching point data when a newer image load fails after a successful mesh commit', async () => {
  const errors = [];
  const initial = mesh();
  const accepted = mesh(null, 'BLEND');
  const broken = mesh('broken.png');
  const renderer = {
    setMaterialMesh: data => data === broken
      ? Promise.reject(new Error('Image load failed'))
      : Promise.resolve(true)
  };
  const viewport = Object.assign(Object.create(Viewport.prototype), {
    renderer, canvas: {dataset: {}}, data: initial, dataRevision: 0, disposed: false, camera: new OrbitCamera(),
    invalidate() {}, onError: message => errors.push(message)
  });

  viewport.setData(accepted, false);
  viewport.setData(broken, false);
  await new Promise(resolve => setImmediate(resolve));

  assert.equal(errors.length, 1);
  assert.equal(viewport.data, accepted);
});
