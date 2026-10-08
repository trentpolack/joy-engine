import test from 'node:test';
import assert from 'node:assert/strict';
import { compile } from 'joy-engine/form';
import { readDocument } from '../src/document.ts';
import { OrbitCamera } from 'joy-engine';
import { pointBatch } from '../src/rendering/batches.ts';

const steel = { name: 'Steel', baseColor: [0.7, 0.8, 0.9, 0.5], metallic: 1, roughness: 0.2, emissive: [0.1, 0, 0], alphaMode: 'BLEND', alphaCutoff: 0.5 };

test('named material selection assigns triangles and points without changing vertex colors', () => {
  const data = compile('material(Steel); color(1,1,1); box(0,0,0,1,1,1); point(0,2,0); material(); point(1,2,0)', {}, [steel]);
  assert.equal(data.materials[1].name, 'Steel');
  assert.deepEqual(data.triangleMaterials, Array(12).fill(1));
  assert.deepEqual(data.pointMaterials, [1, 0]);
  assert.equal(data.colors[0], 1);
});

test('material selection errors retain source locations and do not coerce expressions', () => {
  for(const source of ['material(Missing)', 'material(1)', 'material(Steel, Steel)']) {
    assert.throws(() => compile(source, {}, [steel]), error => error.line === 1 && /material/i.test(error.message));
  }
});

test('document materials survive saves and malformed records are rejected', () => {
  const doc = { version: 1, name: 'Materials', source: 'material(Steel)', overrides: {}, materials: [steel] };
  assert.equal(readDocument(JSON.stringify(doc)).materials.at(-1).name, 'Steel');
  assert.throws(() => readDocument(JSON.stringify({ ...doc, materials: [{ ...steel, metallic: 2 }] })));
});

test('point markers preserve material tint, emission and alpha modes', () => {
  const source = 'material(Steel); color(1,1,1,0.5); point(0,0,0)';
  const camera = new OrbitCamera();
  const preview = material => pointBatch(compile(source, {}, [material]), false, camera, 2, 800);
  const result = preview(steel);
  assert.equal(result[6], 0.25);
  assert.notDeepEqual(result, preview({...steel, emissive: [0,0,0]}));
  assert.equal(preview({...steel, alphaMode: 'OPAQUE'})[6], 1);
  assert.equal(preview({...steel, alphaMode: 'MASK'})[6], 0);
});

