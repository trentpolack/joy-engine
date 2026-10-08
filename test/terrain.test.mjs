import test from 'node:test';
import assert from 'node:assert/strict';
import { decodeTerrain, sampleTerrain } from '../src/rendering/terrain/terrain-asset.ts';
import { selectTerrain, reconcileTerrainEdges, sampleMorphedHeight } from '../src/rendering/terrain/terrain-selection.ts';
import { createTerrainGrid } from '../src/rendering/terrain/terrain-grid.ts';

function fixture() {
  const size = 17, data = new Uint16Array(size*size);
  for(let z = 0; z < size; z++) {
    for(let x = 0; x < size; x++) { data[z*size + x] = x*x + z*z; }
  }
  const manifest = {version:1,size,tileCells:8,extent:[16,16],heightOffset:0,heightScale:65535,heightFile:'height.bin',orientation:'row-0-to-negative-z',patches:[]};
  for(let z = 0; z < 2; z++) {
    for(let x = 0; x < 2; x++) { manifest.patches.push({x:x*8,z:z*8,min:0,max:512,errors:[0,2,8,32]}); }
  }
  return decodeTerrain(manifest, data.buffer);
}
test('asset validates binary length, scale and stable collision triangle sampling', () => {
  const asset = fixture();
  assert.equal(sampleTerrain(asset,-8,-8),0);
  assert.equal(sampleTerrain(asset,8,8),512);
  assert.equal(sampleTerrain(asset,-7.5,-8),0.5);
  assert.throws(() => decodeTerrain({...asset.manifest,extent:[0,16]},asset.heights.buffer));
  assert.throws(() => decodeTerrain(asset.manifest,new ArrayBuffer(2)));
});
test('selection is bounded, coarse at distance and finest near a stable target', () => {
  const asset = fixture();
  const far = selectTerrain(asset,{eye:[100000,100000,100000],projectionScale:1000,tolerance:2,target:[0,0,0],targetRadius:0});
  assert.equal(far.length,4);
  assert.ok(far.every(p => p.lod === 3));
  const close = selectTerrain(asset,{eye:[0,600,0],projectionScale:1000,tolerance:2,target:[-4,0,-4],targetRadius:1});
  assert.equal(close[0].detail,0);
});
test('shared edges agree across arbitrary neighboring levels and transition boundaries', () => {
  const asset = fixture();
  for(const detail of [0,0.3,0.99999,1,1.8,2.99999,3]) {
    const patches = reconcileTerrainEdges(asset,[detail,2.6,0.2,1.2]);
    for(let z = 0; z <= 8; z+= 1) {
      assert.ok(Math.abs(sampleMorphedHeight(asset,0,8,z,patches[0]) - sampleMorphedHeight(asset,1,0,z,patches[1])) < 1e-6);
    }
    const before = reconcileTerrainEdges(asset,[0.999999,0,0,0])[0];
    const after = reconcileTerrainEdges(asset,[1,0,0,0])[0];
    assert.ok(Math.abs(sampleMorphedHeight(asset,0,3,3,before) - sampleMorphedHeight(asset,0,3,3,after)) < 1e-4);
  }
});
test('regular indexed grids share vertices and valid draw ranges at every resolution', () => {
  const grid = createTerrainGrid(8);
  assert.equal(grid.positions.length,81*2);
  assert.equal(grid.ranges.length,4);
  assert.equal(grid.ranges[0].count,8*8*6);
  assert.equal(grid.ranges[3].count,6);
  assert.ok(grid.indices.every(i => i < 81));
});

test('target detail lock fades continuously instead of popping at its boundary', () => {
  const asset = fixture();
  const view = {eye:[10000,10000,10000],projectionScale:1000,tolerance:2,target:[-4,0,-4],targetRadius:1};
  const inside = selectTerrain(asset,{...view,target:[1 - 1e-6,0,-4]})[0];
  const outside = selectTerrain(asset,{...view,target:[1 + 1e-6,0,-4]})[0];
  assert.ok(Math.abs(inside.detail - outside.detail) < 0.001);
  assert.ok(selectTerrain(asset,{...view,target:[1.5,0,-4]})[0].detail > 0);
});

test('invalid view vectors and nonfinite radius fail before selection', () => {
  const asset = fixture();
  const view = {eye:[0,10,0],projectionScale:100,tolerance:2,target:[0,0,0],targetRadius:1};
  assert.throws(() => selectTerrain(asset,{...view,targetRadius:NaN}),/Invalid terrain view/);
  assert.throws(() => selectTerrain(asset,{...view,eye:[0,10]}),/Invalid terrain view/);
  assert.throws(() => selectTerrain(asset,{...view,target:new Array(3)}),/Invalid terrain view/);
  const size = 25;
  assert.throws(() => decodeTerrain({...asset.manifest,size},new ArrayBuffer(size*size*2)),/Invalid terrain/);
});

test('removed vertices converge to stitched parent triangles beside coarser edges', () => {
  const asset = fixture();
  for(const level of [1,2,3]) {
    for(const neighbor of [level,Math.min(3,level + 0.6),3]) {
      const before = reconcileTerrainEdges(asset,[neighbor,level - 1e-7,3,3])[1];
      const after = reconcileTerrainEdges(asset,[neighbor,level,3,3])[1];
      for(let z = 1; z < 8; z++) {
        for(let x = 1; x < 8; x++) {
          const stride = 2**level, bx = Math.floor(x/stride)*stride, bz = Math.floor(z/stride)*stride;
          const fx = (x - bx)/stride, fz = (z - bz)/stride;
          const a = sampleMorphedHeight(asset,1,bx,bz,after);
          const b = sampleMorphedHeight(asset,1,bx + stride,bz,after);
          const c = sampleMorphedHeight(asset,1,bx,bz + stride,after);
          const d = sampleMorphedHeight(asset,1,bx + stride,bz + stride,after);
          const parentPlane = fx >= fz ? a + (b - a)*fx + (d - b)*fz : a + (d - c)*fx + (c - a)*fz;
          assert.ok(Math.abs(sampleMorphedHeight(asset,1,x,z,before) - parentPlane) < 1e-4,`level ${level}, neighbor ${neighbor}, vertex ${x},${z}`);
        }
      }
    }
  }
});
