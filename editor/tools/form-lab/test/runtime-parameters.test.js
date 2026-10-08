import test from 'node:test';
import assert from 'node:assert/strict';
import { compile } from 'joy-engine/form';
import { readDocument } from '../src/document.ts';
import { exportGlb } from '../src/export-glb.ts';

const recipe = `
param count = 3 meta(min=1, max=20, step=1)
param origin = vector(0, 0, 0) meta(min=-20, max=20, step=0.1)
param tint = rgba(0.3, 0.6, 0.9, 1)
param piece = meshBox(1, 2, 1)
param finish = materialPbr(tint, 0.1, 0.7)
repeat count as i {
  instance(piece, vector(origin.x + i * 2, origin.y, origin.z), vector(0, 0, 0), vector(1, 1, 1), finish)
}`;

test('one recipe accepts all five parameter types and round trips resource overrides', () => {
  const initial = compile(recipe);
  assert.deepEqual(initial.parameters.map(p => p.type), ['scalar', 'vector', 'color', 'mesh', 'material']);
  assert.equal(initial.positions.length, 3*8*3);
  const piece = compile('param piece = meshSphere(0.5, 6)').parameters[0].value;
  const overrides = { count: 2, origin: [4, 0, 0], tint: [1, 0, 0, 1], piece };
  const doc = readDocument(JSON.stringify({version: 1, name: 'Bridge', source: recipe, overrides}));
  const result = compile(doc.source, doc.overrides);
  assert.equal(result.triangles.length, 2*24*3);
  assert.deepEqual(result.materials.at(-1).baseColor, [1, 0, 0, 1]);
  assert.ok(exportGlb(result).byteLength > 100);
  const saved = JSON.stringify(overrides);
  result.parameters.find(p => p.name === 'piece').value.positions[0] = 999;
  assert.equal(JSON.stringify(overrides), saved);
  assert.deepEqual(compile(recipe, overrides), compile(recipe, overrides));
});

test('mesh manipulation is local and resource boundaries reject malformed geometry', () => {
  const result = compile(`let base = meshBox(2, 2, 2)
let bent = deform(base, x, y + x*x, z)
let moved = transformMesh(bent, vector(5,0,0), vector(0,0,0), vector(2,1,1))
instance(base)
instance(moved)`);
  assert.deepEqual(result.positions.slice(0, 3), [-1,-1,-1]);
  assert.deepEqual(result.positions.slice(24, 27), [3,0,-1]);
  const invalid = {kind:'mesh',name:'Broken',positions:[0,0,0],normals:[0,1,0],uvs:[0,0],indices:[0,1,2]};
  assert.throws(() => compile(recipe, {piece:invalid}), /index|indices/i);
  assert.throws(() => compile(recipe, {piece:3}), /mesh/i);
  assert.throws(() => compile('repeat 20000 as i { instance(meshBox(1,1,1)) }'), /limit|budget/i);
});

test('a parsed runtime recipe evaluates caller snapshots without retaining overrides or previous output', async () => {
  const {compileFormScript, meshBox, transformMesh} = await import('joy-engine/form');
  const program = compileFormScript('project meta(name="Original")\n' + recipe);
  const overrides = {count: 1, origin: [3, 0, 0]};
  const first = program.evaluate(overrides);
  first.project.name = 'Changed';
  assert.equal(program.evaluate().project.name, 'Original');
  overrides.origin[0] = 8;
  assert.equal(first.positions[0], 2.5);
  assert.equal(program.evaluate(overrides).positions[0], 7.5);
  assert.equal(program.evaluate().positions.length, 72);
  assert.throws(() => program.evaluate({count: NaN}), /finite/);
  assert.equal(first.positions[0], 2.5);
  const mesh = meshBox(1,1,1);
  mesh.normals.fill(0);
  mesh.normals.splice(0, 3, 1, 1, 0);
  const transformed = transformMesh(mesh, [0,0,0], [0,0,0], [-2,1,1]);
  assert.deepEqual(transformed.indices.slice(0,3), [0,1,2]);
  assert.ok(Math.abs(transformed.normals[0] + 1/Math.sqrt(5)) < 1e-12);
  assert.throws(() => transformMesh(mesh, [0,0,0], [0,0,0], [0,1,1]), /nonzero/);
});

test('FORM-exported GLB geometry can return through a runtime mesh parameter', async () => {
  const {parseGlbMesh} = await import('joy-engine');
  const {readMesh} = await import('joy-engine/form');
  const decoded = await parseGlbMesh(exportGlb(compile('sphere(0,0,0,0.5,6)')));
  const piece = readMesh({...decoded, kind:'mesh', uvs:new Array(decoded.positions.length/3*2).fill(0)});
  assert.equal(compile(recipe, {piece,count:1}).triangles.length, 24*3);
});
