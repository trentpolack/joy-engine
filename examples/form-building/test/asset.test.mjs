// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {compile, compileFormScript, materialPbr, meshBox} from 'joy-engine/form';
import {RenderScene, MeshAsset} from 'joy-engine';
import {assetFromEvaluation} from '../src/asset.ts';
import {populateBuildingScene} from '../src/presentation.ts';

const source = readFileSync(new URL('../assets/building.form',import.meta.url),'utf8');
const presets = JSON.parse(readFileSync(new URL('../assets/presets.json',import.meta.url),'utf8'));
const program = compileFormScript(source);

function validGeometry(result) {
  const count = result.positions.length/3;
  assert.ok(count > 0);
  assert.ok(result.positions.every(Number.isFinite));
  assert.ok(result.triangles.every(index => Number.isInteger(index) && index >= 0 && index < count));
  assert.equal(result.triangleMaterials.length,result.triangles.length/3);
  assert.ok(result.triangleMaterials.every(index => result.materials[index]));
}

test('presets, extremes, fractional counts, seed determinism, and material overrides', () => {
  const cases = {...presets,
    minimum:{width:4,depth:4,height:3,floors:1,window_spacing:6,door_spacing:12,roof_rise:0.3},
    maximum:{width:30,depth:30,height:30,floors:8,window_spacing:1.5,door_spacing:3,roof_rise:5},
    fractional:{floors:2.75,roof_shape:0.75},
    compressed:{width:4,depth:4,height:3,floors:8,window_spacing:1.5,door_spacing:3},
    clamped:{width:-100,depth:999,floors:999}};
  for(const [name,overrides] of Object.entries(cases)) {
    const start = performance.now();
    const result = program.evaluate(overrides);
    const milliseconds = performance.now() - start;
    validGeometry(result);
    assert.deepEqual(result,program.evaluate(overrides),name);
    console.info(JSON.stringify({case:name,milliseconds,vertices:result.positions.length/3,triangles:result.triangles.length/3}));
  }
  assert.notDeepEqual(program.evaluate({random_seed:42}).colors,program.evaluate({random_seed:43}).colors);
  const finish = materialPbr([0.1,0.7,0.3,1],0.65,0.12);
  const result = program.evaluate({wall_finish:finish,window_piece:meshBox(0.6,1,0.2)});
  const material = result.materials.find(item => item.baseColor[1] === 0.7);
  assert.equal(material.metallic,0.65);
  assert.equal(material.roughness,0.12);
  const asset = assetFromEvaluation(result);
  const baked = new MeshAsset(JSON.parse(JSON.stringify(asset.geometry)));
  assert.deepEqual(baked.geometry.materials,asset.geometry.materials);
  assert.deepEqual(baked.geometry.triangleMaterials,asset.geometry.triangleMaterials);
  assert.deepEqual(baked.bounds,asset.bounds);
});

test('diagnostics retain locations and failed evaluation does not corrupt a previous generation', () => {
  const previous = program.evaluate();
  const copy = structuredClone(previous);
  for(const overrides of [{width:NaN},{floors:Infinity},{wall_tint:[1,2,3]},{window_piece:3}]) {
    assert.throws(() => program.evaluate(overrides),error => Number.isInteger(error.line) && Number.isInteger(error.column));
  }
  for(const repro of ['box(0,0,0,0,1,1)','repeat 20000 as i { instance(meshBox(1,1,1)) }','let x = nope(1)','if(1 or 0) {point(0,0,0)}']) {
    assert.throws(() => compile(repro),error => Number.isInteger(error.line) && Number.isInteger(error.column));
  }
  assert.deepEqual(previous,copy);
  assert.deepEqual(previous,program.evaluate());
});

test('100 replacements keep four material groups and independent immutable generations', () => {
  const scene = new RenderScene();
  const first = assetFromEvaluation(program.evaluate());
  const timings = [];
  for(let index = 0; index < 100; index++) {
    const start = performance.now();
    const asset = assetFromEvaluation(program.evaluate({...presets.apartments,random_seed:index + 1}));
    populateBuildingScene(scene,asset);
    timings.push(performance.now() - start);
    assert.equal(scene.instances.size,4);
  }
  scene.destroy();
  scene.destroy();
  assert.equal(scene.instances.size,0);
  assert.throws(() => scene.createMesh(first),/destroyed/);
  assert.deepEqual(first.geometry,assetFromEvaluation(program.evaluate()).geometry);
  timings.sort((a,b) => a - b);
  console.info(JSON.stringify({regenerations:100,p50Milliseconds:timings[49],p95Milliseconds:timings[94],maximumMilliseconds:timings[99]}));
});
