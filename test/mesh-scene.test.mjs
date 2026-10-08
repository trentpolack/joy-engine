// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { MeshAsset } from '../src/rendering/scene/mesh-asset.ts';
import { RenderScene } from '../src/rendering/scene/render-scene.ts';
import { prepareSceneFrame } from '../src/rendering/scene/prepare-frame.ts';

function triangle() {
  return new MeshAsset({positions: [0, 0, 0, 1, 0, 0, 0, 1, 0], indices: [0, 1, 2]});
}

test('mesh assets own immutable attributes and metadata independently of instances', () => {
  const source = {positions: [0, 0, 0, 1, 0, 0, 0, 1, 0], indices: [0, 1, 2],
    normals: [0, 0, 1, 0, 0, 1, 0, 0, 1], uvs: [0, 0, 1, 0, 0, 1],
    materials: [{name: 'authored'}], metadata: {outputs: [{name: 'hinge'}]}};
  const asset = new MeshAsset(source);
  source.positions[0] = 99;
  source.metadata.outputs[0].name = 'changed';
  assert.equal(asset.geometry.positions[0], 0);
  assert.equal(asset.geometry.metadata.outputs[0].name, 'hinge');
  assert.equal(asset.geometry.normals.length, 9);
  assert.equal(asset.geometry.uvs.length, 6);
  assert.ok(Object.isFrozen(asset.geometry.materials[0]));
  assert.throws(() => new MeshAsset({...source, metadata: {mutable: new Map()}}), /plain/);
  const cyclic = {};
  cyclic.self = cyclic;
  const cyclicAsset = new MeshAsset({...source, metadata: cyclic});
  assert.ok(Object.isFrozen(cyclicAsset.geometry.metadata.self));
  const first = new RenderScene();
  const second = new RenderScene();
  const instance = second.createMesh(asset);
  first.createMesh(asset);
  first.destroy();
  assert.equal(instance.asset, asset);
  assert.equal(second.instances.size, 1);
  assert.throws(() => first.createMesh(asset), /destroyed/);
  assert.throws(() => new MeshAsset({positions: [0, 0, NaN], indices: [0, 0, 0]}), /finite/);
  assert.throws(() => new MeshAsset({positions: [0, 0, 0], indices: [0, 1, 0]}), /index/i);
});

test('keyed collections retain identity and prune removed entities without growing the scene', () => {
  const scene = new RenderScene();
  const meshes = scene.createCollection();
  const asset = triangle();
  meshes.beginUpdate();
  const first = meshes.set('hero', asset, {position: [1, 2, 3]});
  meshes.set('enemy', asset);
  meshes.endUpdate();
  meshes.beginUpdate();
  assert.equal(meshes.set('hero', asset, {position: [4, 5, 6]}), first);
  meshes.endUpdate();
  assert.equal(scene.instances.size, 1);
  assert.deepEqual(first.position, [4, 5, 6]);
  meshes.clear();
  assert.equal(scene.instances.size, 0);
  scene.destroy();
  scene.destroy();
});

test('frame preparation applies full transforms and preserves shared local geometry', () => {
  const scene = new RenderScene();
  const asset = triangle();
  const instance = scene.createMesh(asset, {position: [1, 2, 3], scale: [-2, 3, 1], rotation: [0, Math.PI/2, 0], tint: [2, 0.5, 1]});
  const frame = prepareSceneFrame(scene, {backward: [0, 0, 1]});
  assert.deepEqual(frame.opaque.slice(0, 7), [1, 2, 3, 2, 0.5, 1, 1]);
  assert.ok(Math.abs(frame.opaque[7] - 1) < 1e-12);
  assert.equal(frame.opaque[9], 5);
  assert.deepEqual(asset.geometry.positions.slice(0, 6), [0, 0, 0, 1, 0, 0]);
  instance.position[0] = 8;
  assert.equal(prepareSceneFrame(scene, {backward: [0, 0, 1]}).opaque[0], 8);
  instance.visible = false;
  assert.equal(prepareSceneFrame(scene, {backward: [0, 0, 1]}).opaque.length, 0);
});

test('frustum culling keeps intersecting bounds and sorts translucent geometry globally', () => {
  const scene = new RenderScene();
  const asset = triangle();
  scene.createMesh(asset, {position: [20, 0, 0]});
  scene.createMesh(asset, {position: [0.9, 0, 0], opacity: 0.5});
  scene.createMesh(asset, {position: [0, 0, -0.5], opacity: 0.5});
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const frame = prepareSceneFrame(scene, {backward: [0, 0, 1], viewProjection: identity, webgpu: false});
  assert.equal(frame.opaque.length, 0);
  assert.equal(frame.transparent.length, 42);
  assert.equal(frame.transparent[2], -0.5);
  assert.equal(frame.culled, 1);
  assert.equal(frame.visible, 2);
  assert.equal(prepareSceneFrame(scene, {backward: [0, 0, 1], viewProjection: identity, webgpu: true}).transparent.length, 21);
});
