// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import { TerrainRenderer } from '../src/rendering/terrain/terrain-renderer.ts';
import { decodeTerrain } from '../src/rendering/terrain/terrain-asset.ts';
import { TerrainScenePass } from '../src/rendering/terrain/terrain-scene-pass.ts';
function asset() {
  return decodeTerrain({version:1,size:3,tileCells:2,extent:[2,2],heightOffset:0,heightScale:1,heightFile:'height.bin',orientation:'row-0-to-negative-z',patches:[{x:0,z:0,min:0,max:0,errors:[0,0]}]},new ArrayBuffer(18));
}
function device(failAt = Infinity) {
  let allocations = 0;
  const resources = [], calls = [];
  function resource(extra = {}) {
    if(++allocations === failAt) { throw new Error('Allocation failure'); }
    const result = {destroyCount:0,destroy(){this.destroyCount++;},...extra};
    resources.push(result); return result;
  }
  return {type:'webgl',info:{shadingLanguage:'glsl',gpu:'unknown'},features:new Set(),limits:{},resources,calls,
    createShader:() => resource(),
    createRenderPipeline:props => resource({shaderLayout:{attributes:[],bindings:[]},bufferLayout:props.bufferLayout}),
    createVertexArray:() => resource({setBuffer(){},setIndexBuffer(){}}),
    createBuffer:props => resource({...props,write(data){calls.push({kind:'upload',bytes:data.byteLength,id:props.id,values:Array.from(data)});}}),
    createTexture:() => resource({copyImageData(){}})};
}
test('terrain construction rolls back every partial GPU allocation', () => {
  for(let failure = 1; failure <= 9; failure++) {
    const gpu = device(failure);
    assert.throws(() => new TerrainRenderer(gpu,asset()),/Allocation failure/);
    assert.ok(gpu.resources.every(resource => resource.destroyCount === 1));
  }
});

test('native terrain retains indexed resources across transforms and unwinds partial construction', () => {
  const gpu = device();
  const native = new TerrainScenePass(gpu, asset());
  const count = gpu.resources.length;
  const transform = [2,0,0, 0,2,0, 0,0,2, 8,4,-6];
  const view = {eye:[10,8,-4],target:[8,4,-6],projectionScale:100,tolerance:2,targetRadius:0};
  native.prepare(new Float32Array(16), transform, view);
  assert.deepEqual(Array.from(native.terrain.frameData.slice(16,20)), [1,2,1,100], 'LOD eye uses local coordinates and uniform scale keeps screen error invariant');
  const draws = [];
  const pass = {setPipeline(){},setVertexArray(){},setBindings(){},draw(value){draws.push(value);}};
  for(let index = 0; index < 10; index++) {
    transform[9]+= 1;
    native.prepare(new Float32Array(16), transform, view);
    native.draw(pass, {uniforms:{}}, false, false);
  }
  assert.equal(gpu.resources.length, count);
  assert.ok(draws.every(draw => draw.indexCount > 0 && !('vertexCount' in draw)), 'scene path must retain indexed draws');
  native.prepare(new Float32Array(16), Array(12).fill(0), view);
  assert.ok(native.terrain.frameData.every(Number.isFinite), 'collapsed transforms never feed infinities into LOD selection');
  const uploads = gpu.calls.length;
  assert.throws(() => native.prepare(new Float32Array(16), [...transform.slice(0,11),NaN], view), /finite affine/);
  assert.equal(gpu.calls.length, uploads);
  native.destroy(); native.destroy();
  assert.ok(gpu.resources.every(resource => resource.destroyCount === 1));
  for(let failure = 1; failure <= count; failure++) {
    const failing = device(failure);
    assert.throws(() => new TerrainScenePass(failing, asset()), /Allocation failure/);
    assert.ok(failing.resources.every(resource => resource.destroyCount === 1));
  }
});

test('resident terrain records indexed geometry in color, shadow, surface, and motion passes', () => {
  const draws = [];
  const uploads = [];
  const pipeline = name => ({name});
  const adapter = Object.assign(Object.create(TerrainScenePass.prototype), {
    terrain: {
      drawGeometry(pass, selectedPipeline, bindings) {
        draws.push({pass, pipeline: selectedPipeline.name, bindings});
      }
    },
    model: {name: 'model'},
    opaque: pipeline('color'),
    shadowed: pipeline('shadow-color'),
    additive: pipeline('shadow-additive'),
    caster: pipeline('shadow-depth'),
    surface: pipeline('surface'),
    motion: pipeline('motion'),
    surfaceUniforms: {write(data) { uploads.push({kind: 'surface', data: Array.from(data)}); }},
    motionUniforms: {write(data) { uploads.push({kind: 'motion', data: Array.from(data)}); }},
    currentModel: new Float32Array(16).fill(2),
    previousModel: new Float32Array(16).fill(1)
  });
  const pass = {};
  const matrix = new Float32Array(16).fill(3);

  adapter.draw(pass, {uniforms: 'scene'}, false, false);
  adapter.drawShadow(pass, {name: 'light'});
  adapter.drawSurface(pass, matrix, matrix);
  adapter.drawMotion(pass, {
    currentViewProjection: matrix,
    previousViewProjection: matrix,
    rasterViewProjection: matrix,
    velocityJitterDelta: [0.25, -0.25]
  });

  assert.deepEqual(draws.map(draw => draw.pipeline), ['color', 'shadow-depth', 'surface', 'motion']);
  assert.equal(draws.every(draw => draw.pass === pass), true);
  assert.equal(draws[0].bindings.terrainModel.name, 'model');
  assert.equal(draws[1].bindings.casterCamera.name, 'light');
  assert.equal(draws[2].bindings.attachment, adapter.surfaceUniforms);
  assert.equal(draws[3].bindings.attachment, adapter.motionUniforms);
  assert.deepEqual(uploads.map(upload => upload.kind), ['surface', 'motion']);
  assert.deepEqual(uploads[1].data.slice(80, 82), [0.25, -0.25]);
});
test('terrain frames retain resources, use WebGL indexed byte offsets and release once', () => {
  const gpu = device(), terrain = new TerrainRenderer(gpu,asset());
  const allocations = gpu.resources.length;
  const draws = [];
  const pass = {setPipeline(){},setVertexArray(){},setBindings(){},draw(options){draws.push(options);}};
  for(let i = 0; i < 20; i++) {
    terrain.prepare(new Float32Array(16),{eye:[1000,1000,1000],projectionScale:100,tolerance:2,target:[0,0,0],targetRadius:0});
    terrain.draw(pass);
  }
  assert.equal(gpu.resources.length,allocations);
  assert.equal(draws[0].indexCount,6);
  assert.equal(draws[0].firstVertex,24*4);
  terrain.destroy(); terrain.destroy();
  assert.ok(gpu.resources.every(resource => resource.destroyCount === 1));
  assert.throws(() => terrain.prepare(new Float32Array(16),{eye:[0,0,0],projectionScale:100,tolerance:2,target:[0,0,0],targetRadius:0}),/disposed/);
});

test('timestamp samples are asynchronous, serialized and release their query owner', async () => {
  const { TerrainTiming } = await import('../src/rendering/terrain/terrain-timing.ts');
  let release;
  let destroyed = 0;
  const query = {destroy(){destroyed++;},readTimestampDuration:() => new Promise(resolve => { release = resolve; })};
  const gpu = {features:new Set(['timestamp-query']),createQuerySet:() => query};
  const timer = new TerrainTiming(gpu);
  assert.equal(timer.begin(1000).beginTimestampIndex,0);
  const pending = timer.end();
  assert.deepEqual(timer.begin(2000),{});
  release(2.5); await pending;
  assert.equal(timer.milliseconds,2.5);
  assert.equal(timer.begin(2500).endTimestampIndex,1);
  timer.destroy(); timer.destroy(); assert.equal(destroyed,1);
});

test('both renderer paths reject invalid inputs before uploading or dispatching', () => {
  const gpu = device(), terrain = new TerrainRenderer(gpu,asset());
  const view = {eye:[0,10,0],projectionScale:100,tolerance:2,target:[0,0,0],targetRadius:0};
  for(const backend of ['webgl','webgpu']) {
    gpu.type = backend;
    const calls = gpu.calls.length;
    assert.throws(() => terrain.prepare(new Float32Array(16),{...view,targetRadius:NaN}),/Invalid terrain view/);
    assert.throws(() => terrain.prepare(new Float32Array(15),view),/Invalid terrain matrix/);
    assert.equal(gpu.calls.length,calls);
  }
  terrain.destroy();
});
