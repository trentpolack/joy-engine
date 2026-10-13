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
  assert.equal(frame.shadowCasters.length, 21, 'camera-culled opaque geometry still casts shadows');
  assert.equal(frame.culled, 1);
  assert.equal(frame.visible, 2);
  assert.equal(prepareSceneFrame(scene, {backward: [0, 0, 1], viewProjection: identity, webgpu: true}).transparent.length, 21);
});

test('hierarchical transforms retain shear and direct parent edits update geometry and culling', () => {
  const scene = new RenderScene();
  const parent = scene.createNode({position: [0.2, 0, 0], rotation: [0, 0, Math.PI/2], scale: [2, 1, 1]});
  const child = scene.createMesh(triangle(), {parent, rotation: [0, 0, Math.PI/4]});
  const frame = prepareSceneFrame(scene, {backward: [0, 0, 1]});
  const rootTwo = Math.sqrt(2);
  assert.ok(Math.abs(frame.opaque[7] - (0.2 - 1/rootTwo)) < 1e-12);
  assert.ok(Math.abs(frame.opaque[8] - rootTwo) < 1e-12);
  assert.ok(Math.abs(frame.opaque[14] - (0.2 - 1/rootTwo)) < 1e-12);
  assert.ok(Math.abs(frame.opaque[15] + rootTwo) < 1e-12);
  const world = child.worldTransform;
  assert.ok(Math.abs(world[0]*world[3] + world[1]*world[4] + 1.5) < 1e-12, 'shear cannot be replaced by decomposed XYZ scale');
  const previousRevision = child.worldRevision;
  parent.position[0] = 20;
  assert.equal(parent.transformRevision, 2, 'reading the own revision must not consume a pending world update');
  assert.equal(child.worldTransform[9], 20);
  assert.ok(child.worldRevision > previousRevision);
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const moved = prepareSceneFrame(scene, {backward: [0, 0, 1], viewProjection: identity});
  assert.equal(moved.visible, 0);
  assert.equal(moved.culled, 1);
  assert.equal(moved.shadowCasters[0], 20);
  assert.equal(parent.transformRevision, 2);
  parent.set({position: [0, 0, 0], rotation: [0, 0, 0], scale: [-2, 0, 1]});
  const flattened = prepareSceneFrame(scene, {backward: [0, 0, 1]});
  assert.ok(flattened.opaque.every(Number.isFinite), 'nested zero scale never requires affine inversion');
  assert.ok(Math.abs(flattened.opaque[7] + rootTwo) < 1e-12);
  assert.equal(flattened.opaque[8], 0);
  assert.ok(Math.abs(flattened.opaque[14] - rootTwo) < 1e-12);
});

test('parenting rejects foreign, removed and cyclic nodes atomically while preserving local transforms', () => {
  const scene = new RenderScene();
  const first = scene.createNode({position: [5, 0, 0]});
  const child = scene.createNode({position: [1, 2, 3]}, first);
  const grandchild = scene.createNode({}, child);
  const foreign = new RenderScene().createNode();
  assert.throws(() => first.setParent(grandchild), /cycle/i);
  assert.throws(() => child.setParent(foreign), /scene/i);
  assert.equal(child.parent, first);
  assert.ok(first.children.has(child));
  child.setParent(null);
  assert.deepEqual(child.position, [1, 2, 3]);
  assert.deepEqual(child.worldTransform.slice(9), [1, 2, 3]);
  assert.ok(!first.children.has(child));
  scene.remove(first);
  assert.throws(() => child.setParent(first), /removed/i);
  assert.equal(child.parent, null);
  const count = scene.nodes.size;
  assert.throws(() => scene.createMesh(triangle(), {parent: foreign}), /scene/i);
  assert.equal(scene.nodes.size, count);
  const mesh = scene.createMesh(triangle(), {position: [1, 2, 3]});
  assert.throws(() => mesh.set({parent: foreign, position: [99, 0, 0]}), /scene/i);
  assert.deepEqual(mesh.position, [1, 2, 3]);
  assert.equal(mesh.parent, null);
});

test('hidden ancestors suppress descendants and subtree removal invalidates collection entries without releasing assets', () => {
  const scene = new RenderScene();
  const asset = triangle();
  const group = scene.createNode();
  const collection = scene.createCollection();
  collection.beginUpdate();
  const mesh = collection.set('child', asset, {parent: group});
  const descendant = scene.createMesh(asset, {parent: mesh});
  collection.endUpdate();
  group.visible = false;
  assert.equal(mesh.worldVisible, false);
  assert.equal(prepareSceneFrame(scene, {backward: [0, 0, 1]}).shadowCasters.length, 0);
  group.visible = true;
  assert.equal(prepareSceneFrame(scene, {backward: [0, 0, 1]}).visible, 2);
  scene.remove(group);
  assert.equal(scene.nodes.size, 0);
  assert.equal(scene.instances.size, 0);
  assert.equal(mesh.removed, true);
  assert.equal(descendant.removed, true);
  assert.equal(mesh.asset, asset);
  collection.beginUpdate();
  assert.notEqual(collection.set('child', asset), mesh);
  collection.endUpdate();
  assert.equal(scene.instances.size, 1);
  scene.destroy();
  assert.equal(scene.nodes.size, 0);
  assert.equal(asset.geometry.positions.length, 9);
});

test('shadow policy keeps off-camera solid casters independent from opaque receiver runs', () => {
  const scene = new RenderScene();
  const asset = triangle();
  scene.createMesh(asset, {position: [20, 0, 0]});
  scene.createMesh(asset, {castsShadow: false, receivesShadow: false});
  scene.createMesh(asset, {castsShadow: false});
  scene.createMesh(asset, {opacity: 0.5, castsShadow: true});
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const frame = prepareSceneFrame(scene, {backward: [0, 0, 1], viewProjection: identity, opaqueTriangles: Array(21).fill(0)});
  assert.equal(frame.shadowCasters.length, 42);
  assert.equal(frame.shadowCasters[21], 20);
  assert.deepEqual(frame.receivesShadowRanges, [
    {firstVertex: 0, vertexCount: 3, receivesShadow: true},
    {firstVertex: 3, vertexCount: 3, receivesShadow: false},
    {firstVertex: 6, vertexCount: 3, receivesShadow: true}
  ]);
  assert.equal(frame.geometrySnapshots.length, 4);
  assert.equal(frame.geometrySnapshots[0].asset, asset);
  assert.equal(frame.geometrySnapshots[0].worldBounds.min[0], 20);
});
