import test from 'node:test';
import assert from 'node:assert/strict';
import { AssetScene } from '../src/gameplay/asset-scene.ts';
import { copyEntityDefinition } from '../src/gameplay/entity-definition.ts';
import { readFile } from 'node:fs/promises';
const mesh = 'project meta(name="Box")\nbox(0, 0, 0, 2, 2, 2)';
const entity = (id, components) => copyEntityDefinition({id, components});

test('asset scenes share compiled meshes, own effects per instance, and retain their valid scene on failed loads', async () => {
  const fx = await readFile(new URL('./fixtures/fire.joyfx', import.meta.url), 'utf8');
  const files = new Map([['assets/box.form', mesh], ['assets/fire.joyfx', fx]]);
  const scene = new AssetScene({readText: async path => {
    if(!files.has(path)) {
      throw Error('missing');
    }
    return files.get(path);
  }});
  const entities = ['a', 'b'].map(id => entity(id, {render: {asset: 'assets/box.form'}, effect: {asset: 'assets/fire.joyfx', seed: 7777, loop: true}}));
  await scene.sync(entities);
  assert.equal(scene.meshes.get('a'), scene.meshes.get('b'));
  assert.notEqual(scene.effects.get('a').effect, scene.effects.get('b').effect);
  scene.step(0.1);
  assert.ok(scene.particles({right: [1,0,0], up: [0,1,0], backward: [0,0,1]}).length > 0);
  const oldMesh = scene.meshes.get('a');
  files.set('assets/box.form', 'broken(');
  await assert.rejects(scene.sync(entities));
  assert.equal(scene.meshes.get('a'), oldMesh);
  scene.destroy();
  assert.equal(scene.scene.instances.size, 0);
  assert.equal(scene.effects.size, 0);
});

test('late asset reads cannot replace a newer scene or resurrect a disposed owner', async () => {
  let release;
  const scene = new AssetScene({readText: () => new Promise(resolve => {release = resolve;})});
  const first = scene.sync([entity('old', {render: {asset: 'assets/box.form'}})]);
  await new Promise(resolve => setImmediate(resolve));
  await scene.sync([entity('new', {render: {shape: 'box'}})]);
  release(mesh);
  assert.equal(await first, false);
  assert.equal(scene.entities[0].id, 'new');
  const cancelled = scene.sync([entity('cancelled', {render: {asset: 'assets/box.form'}})]);
  await new Promise(resolve => setImmediate(resolve));
  scene.cancelPending();
  release(mesh);
  assert.equal(await cancelled, false);
  assert.equal(scene.entities[0].id, 'new');
  const late = scene.sync([entity('late', {render: {asset: 'assets/box.form'}})]);
  await new Promise(resolve => setImmediate(resolve));
  scene.destroy();
  release(mesh);
  assert.equal(await late, false);
});
