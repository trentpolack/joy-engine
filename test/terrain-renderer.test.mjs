// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import { TerrainRenderer } from '../src/rendering/terrain/terrain-renderer.ts';
import { decodeTerrain } from '../src/rendering/terrain/terrain-asset.ts';
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
    createBuffer:props => resource({byteLength:props.byteLength,write(data){calls.push({kind:'upload',bytes:data.byteLength});}}),
    createTexture:() => resource({copyImageData(){}})};
}
test('terrain construction rolls back every partial GPU allocation', () => {
  for(let failure = 1; failure <= 9; failure++) {
    const gpu = device(failure);
    assert.throws(() => new TerrainRenderer(gpu,asset()),/Allocation failure/);
    assert.ok(gpu.resources.every(resource => resource.destroyCount === 1));
  }
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
