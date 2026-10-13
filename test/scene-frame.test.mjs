// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { OrbitCamera } from '../src/rendering/camera/orbit-camera.ts';
import { screenRay, projectToViewport, intersectRayPlane } from '../src/rendering/camera/projection.ts';
import { SceneRenderer } from '../src/rendering/scene/scene-renderer.ts';
import { EnvironmentSystem } from '../src/environment/environment-system.ts';
import { POINT_LIGHT_UNIFORM_BYTES } from '../src/rendering/pbr/point-lights.ts';
import { RenderScene } from '../src/rendering/scene/render-scene.ts';
import { GpuTriangleRenderer } from '../src/rendering/renderers/gpu-triangle-renderer.ts';
import { createFormMeshAsset } from '../src/form/mesh-asset.ts';

const bounds = {left: 40, top: 30, width: 800, height: 500};
test('projection and picking round-trip CSS coordinates on both clip-depth conventions', () => {
  const camera = new OrbitCamera();
  for(const webgpu of [false, true]) {
    const matrix = camera.matrix(bounds.width/bounds.height, webgpu);
    const projected = projectToViewport([1, 0, 2], matrix, bounds);
    const ray = screenRay(projected.x + bounds.left, projected.y + bounds.top, matrix, bounds, webgpu);
    const hit = intersectRayPlane(ray, [0, 1, 0], 0);
    assert.ok(Math.abs(hit[0] - 1) < 1e-4);
    assert.ok(Math.abs(hit[2] - 2) < 1e-4);
    assert.equal(screenRay(0, 0, matrix, {...bounds, width: 0}, webgpu), null);
  }
  assert.equal(intersectRayPlane({origin: [0, 1, 0], direction: [1, 0, 0]}, [0, 1, 0], 0), null);
});

test('FORM assets preserve authored channels and metadata through the asset boundary', () => {
  const asset = createFormMeshAsset('box(0, 0, 0, 1, 2, 3)');
  assert.ok(asset.geometry.positions.length > 0);
  assert.equal(asset.geometry.normals.length, asset.geometry.positions.length);
  assert.equal(asset.geometry.uvs.length, asset.geometry.positions.length/3*2);
  assert.ok(Array.isArray(asset.geometry.metadata.outputs));
});

test('scene renderer owns initialization, frame submission, resize and late disposal', async () => {
  const original = GpuTriangleRenderer.create;
  let release;
  let destroyed = 0;
  let submitted;
  let creationOptions;
  const resized = [];
  const backend = {backend: 'webgl', resetSceneHistory: () => {}, resize: (...args) => resized.push(args),
    render: (...args) => { submitted = args; }, destroy: () => { destroyed++; }};
  GpuTriangleRenderer.create = async (_canvas, options) => {
    creationOptions = options;
    return backend;
  };
  const canvas = {clientWidth: 800, clientHeight: 500, getBoundingClientRect: () => bounds};
  try {
    const renderer = new SceneRenderer(canvas, {camera: new OrbitCamera(), pixelRatio: 2, atmosphere: true});
    await renderer.initialize();
    assert.deepEqual(creationOptions.postProcessing, {}, 'standard scene viewports use the shared display pipeline');
    const scene = new RenderScene();
    renderer.render(scene, {lights: [], ambient: 0.5, timeSeconds: 3});
    renderer.render(scene, {lights: [], ambient: 0.5, timeSeconds: 4});
    assert.equal(resized.length, 1);
    assert.deepEqual(resized[0], [800, 500, 2]);
    assert.equal(submitted[1].byteLength, POINT_LIGHT_UNIFORM_BYTES);
    assert.equal(submitted[4], 4);
    const environment = new EnvironmentSystem({timeOfDay: 0.5}).sample();
    renderer.render(scene, {environment});
    assert.equal(creationOptions.atmosphere, true);
    assert.equal(submitted[8].byteLength, 352, 'sky receives its own complete uniform block');
    assert.deepEqual(Array.from(submitted[8].slice(20, 22)), [1600, 1000], 'sky uses physical viewport dimensions');
    assert.deepEqual(Array.from(submitted[1].slice(280, 284)), [...environment.primaryLight.direction, environment.primaryLight.intensity].map(Math.fround));
    assert.equal(submitted[1][21], 0, 'environment replaces the fixed ambient default');
    const skyMatrix = submitted[8].slice(0, 16);
    const cameraMatrix = renderer.camera.matrix(1.6, false);
    for(let column = 0; column < 4; column++) {
      for(let row = 0; row < 4; row++) {
        let value = 0;
        for(let axis = 0; axis < 4; axis++) {
          value+= cameraMatrix[axis*4 + row]*skyMatrix[column*4 + axis];
        }
        assert.ok(Math.abs(value - Number(row === column)) < 0.002, 'sky and geometry use inverse camera matrices');
      }
    }
    renderer.render(scene);
    assert.equal(submitted[8], null, 'omitting environment hides the sky on the next frame');
    renderer.destroy();
    renderer.destroy();
    assert.equal(destroyed, 1);
    assert.equal(scene.destroyed, false, 'renderer borrows the scene');
    GpuTriangleRenderer.create = () => new Promise(resolve => { release = resolve; });
    const late = new SceneRenderer(canvas);
    const pending = late.initialize();
    late.destroy();
    release(backend);
    await pending;
    assert.equal(destroyed, 2);
    assert.equal(late.ready, false);
  } finally {
    GpuTriangleRenderer.create = original;
  }
});

test('scene effects expose real object/camera correspondence and reset on discontinuities and failed submission', async () => {
  const original = GpuTriangleRenderer.create;
  const submissions = [];
  let resets = 0;
  let reject = false;
  const backend = {backend: 'webgpu', sceneEffects: {capabilities: {backend: 'webgpu'}},
    resize() {}, destroy() {}, resetSceneHistory() { resets++; }, setSceneEffectsConfig() {},
    render(...args) { if(reject) { throw new Error('submission rejected'); } submissions.push(args); }};
  GpuTriangleRenderer.create = async (_canvas, options) => {
    assert.equal(options.sceneEffects.taa, true);
    assert.ok(options.device.optionalFeatures.includes('float32-filterable'));
    return backend;
  };
  try {
    const camera = new OrbitCamera();
    const canvas = {clientWidth: 800, clientHeight: 500};
    const renderer = new SceneRenderer(canvas, {camera, pixelRatio: 1, sceneEffects: {taa: true}});
    await renderer.initialize();
    const scene = new RenderScene();
    const mesh = scene.createMesh(createFormMeshAsset('box(0, 0, 0, 1, 2, 3)'), {sceneRoughness: 0.2});
    renderer.render(scene, {timeSeconds: 1});
    assert.equal(submissions[0][7].resetHistory, true);
    const initialPositions = Array.from(submissions[0][9].motion.previousPositions);
    mesh.position[0] = 0.1;
    camera.yaw+= 0.01;
    renderer.render(scene, {timeSeconds: 1.016});
    assert.equal(submissions[1][7].resetHistory, false);
    assert.deepEqual(Array.from(submissions[1][9].motion.previousPositions), initialPositions);
    assert.notDeepEqual(Array.from(submissions[1][9].motion.currentViewProjection), Array.from(submissions[1][9].motion.previousViewProjection));
    assert.equal(submissions[1][9].surfaceRoughness[0], 0.2);
    assert.notDeepEqual(submissions[1][7].currentJitter, submissions[1][7].previousJitter);
    renderer.render(scene, {timeSeconds: 0.5});
    assert.equal(submissions[2][7].resetHistory, true, 'rewinding animation invalidates history');
    camera.yaw+= 1;
    renderer.render(scene, {timeSeconds: 0.516});
    assert.equal(submissions[3][7].resetHistory, true, 'camera cuts invalidate history');
    reject = true;
    assert.throws(() => renderer.render(scene, {timeSeconds: 0.532}), /submission rejected/);
    reject = false;
    renderer.render(scene, {timeSeconds: 0.548});
    assert.equal(submissions[4][7].resetHistory, true, 'failed submission never commits CPU history');
    renderer.setSceneEffectsConfig({taa: false});
    renderer.render(scene, {timeSeconds: 0.564});
    assert.equal(submissions[5][7].resetHistory, true, 'toggling starts fresh');
    canvas.clientWidth = 0;
    assert.equal(renderer.render(scene, {timeSeconds: 0.58}), false);
    canvas.clientWidth = 800;
    renderer.render(scene, {timeSeconds: 0.596});
    assert.equal(submissions[6][7].resetHistory, true, 'hidden viewports resume with fresh history');
    assert.ok(resets >= 5);
    renderer.destroy();
    assert.equal(scene.destroyed, false);
  } finally {
    GpuTriangleRenderer.create = original;
  }
});

async function residentTerrainAsset() {
  const {decodeTerrain} = await import('../src/rendering/terrain/terrain-asset.ts');
  return decodeTerrain({version: 1, size: 3, tileCells: 2, extent: [2, 2], heightOffset: 0, heightScale: 1,
    heightFile: 'height.bin', orientation: 'row-0-to-negative-z', patches: [{x: 0, z: 0, min: 0, max: 0, errors: [0, 0]}]}, new ArrayBuffer(18));
}

test('resident terrain rejects missing normal and motion attachments before allocating or resizing GPU resources', async () => {
  const original = GpuTriangleRenderer.create;
  let submitted = 0;
  let resized = 0;
  const backend = {backend: 'webgpu', resetSceneHistory() {}, resize() { resized++; },
    render() { submitted++; }, destroy() {}};
  GpuTriangleRenderer.create = async () => backend;
  try {
    const scene = new RenderScene();
    const parent = scene.createNode();
    scene.createTerrain(await residentTerrainAsset(), {parent});
    for(const [effects, attachment] of [[{ssao: {normalSource: 'normal-texture'}}, 'normalTexture'],
      [{clusteredLighting: true}, 'velocityTexture'], [{taa: true}, 'taaVelocityTexture']]) {
      const renderer = new SceneRenderer({clientWidth: 800, clientHeight: 500}, {sceneEffects: effects});
      await renderer.initialize();
      assert.throws(() => renderer.render(scene), new RegExp(`terrain.*${attachment}`, 'i'));
      assert.equal(submitted, 0);
      assert.equal(resized, 0, 'missing native attachments fail before resize allocations');
      renderer.destroy();
    }
    parent.visible = false;
    const hidden = new SceneRenderer({clientWidth: 800, clientHeight: 500}, {sceneEffects: {taa: true}});
    await hidden.initialize();
    assert.equal(hidden.render(scene), true, 'hidden native geometry contributes no unsupported attachments');
    hidden.destroy();
  } finally {
    GpuTriangleRenderer.create = original;
  }
});

function residentTerrainDevice(backend = 'webgl') {
  const resources = [];
  function resource(extra = {}) {
    const result = {destroyCount: 0, destroy() { this.destroyCount++; }, ...extra};
    resources.push(result);
    return result;
  }
  return {type: backend, info: {shadingLanguage: backend === 'webgpu' ? 'wgsl' : 'glsl', gpu: 'unknown'}, features: new Set(), limits: {}, resources,
    createShader: props => resource({source: props.source}),
    createRenderPipeline: props => resource({props, shaderLayout: {attributes: [], bindings: []}, bufferLayout: props.bufferLayout}),
    createComputePipeline: () => resource({setBindings() {}}),
    beginComputePass: () => ({setPipeline() {}, dispatch() {}, end() {}}),
    createVertexArray: () => resource({setBuffer() {}, setIndexBuffer() {}}),
    createBuffer: props => resource({byteLength: props.byteLength, write() {}}),
    createTexture: () => resource({copyImageData() {}})};
}

test('scene renderer retains native terrain adapters across frames and disposes inactive nodes before the GPU owner', async () => {
  const original = GpuTriangleRenderer.create;
  const gpu = residentTerrainDevice();
  const submissions = [];
  let backendDestroyed = 0;
  const backend = {backend: 'webgl', device: gpu, postprocessor: {format: 'rgba16float'},
    resetSceneHistory() {}, resize() {}, render(...args) { submissions.push(args); },
    destroy() {
      assert.ok(gpu.resources.every(resource => resource.destroyCount === 1), 'native resources must be released before their device owner');
      backendDestroyed++;
    }};
  GpuTriangleRenderer.create = async () => backend;
  try {
    const renderer = new SceneRenderer({clientWidth: 800, clientHeight: 500}, {pixelRatio: 2});
    await renderer.initialize();
    const scene = new RenderScene();
    const parent = scene.createNode();
    const terrain = scene.createTerrain(await residentTerrainAsset(), {parent, receivesShadow: false});
    renderer.render(scene, {culling: false});
    assert.equal(submissions[0][11]?.length, 1);
    assert.equal(submissions[0][11][0].visible, true);
    assert.equal(submissions[0][11][0].castsShadow, true);
    assert.equal(submissions[0][11][0].receivesShadow, false);
    assert.equal(submissions[0][0].length, 0, 'resident terrain does not add CPU triangles');
    assert.deepEqual(submissions[0][10].receivesShadowRanges, []);
    const allocations = gpu.resources.length;
    assert.ok(allocations > 0);
    renderer.camera.yaw+= 0.01;
    renderer.render(scene, {culling: false});
    assert.equal(gpu.resources.length, allocations, 'camera movement retains resident allocations');
    parent.position[0] = 10000;
    renderer.render(scene);
    assert.equal(submissions[2][11].length, 1);
    assert.equal(submissions[2][11][0].visible, false, 'off-camera terrain retains its shadow adapter');
    assert.equal(gpu.resources.length, allocations);
    terrain.castsShadow = false;
    renderer.render(scene);
    assert.equal(submissions[3][11].length, 0);
    assert.ok(gpu.resources.every(resource => resource.destroyCount === 0), 'camera culling retains already resident noncasting terrain');
    parent.position[0] = 0;
    renderer.render(scene, {culling: false});
    assert.equal(submissions[4][11][0].castsShadow, false);
    assert.equal(gpu.resources.length, allocations, 'returning to the camera reuses retained resident resources');
    parent.visible = false;
    renderer.render(scene);
    assert.equal(submissions[5][11].length, 0);
    assert.ok(gpu.resources.every(resource => resource.destroyCount === 1));
    parent.visible = true;
    renderer.render(scene, {culling: false});
    scene.remove(parent);
    renderer.render(scene);
    assert.equal(submissions[7][11].length, 0);
    assert.ok(gpu.resources.every(resource => resource.destroyCount === 1));
    const anotherScene = new RenderScene();
    anotherScene.createTerrain(terrain.asset);
    renderer.render(anotherScene, {culling: false});
    renderer.render(new RenderScene());
    assert.ok(gpu.resources.every(resource => resource.destroyCount === 1), 'switching scenes prunes the previous scene adapters');
    anotherScene.createTerrain(terrain.asset);
    renderer.render(anotherScene, {culling: false});
    renderer.destroy();
    renderer.destroy();
    assert.equal(backendDestroyed, 1);
    assert.equal(anotherScene.terrainInstances.size, 2, 'renderer still borrows scene nodes and assets');
  } finally {
    GpuTriangleRenderer.create = original;
  }
});


test('native terrain permits effects reconstructed from depth without requesting missing surface attachments', async () => {
  const original = GpuTriangleRenderer.create;
  const gpu = residentTerrainDevice('webgpu');
  let submitted;
  const backend = {backend: 'webgpu', device: gpu, postprocessor: {format: 'rgba16float'},
    resetSceneHistory() {}, resize() {}, render(...args) { submitted = args; }, destroy() {}};
  GpuTriangleRenderer.create = async () => backend;
  try {
    const renderer = new SceneRenderer({clientWidth: 800, clientHeight: 500},
      {shadows: false, sceneEffects: {ssao: true}});
    await renderer.initialize();
    const scene = new RenderScene();
    scene.createTerrain(await residentTerrainAsset());
    assert.equal(renderer.render(scene, {culling: false}), true);
    assert.equal(submitted[11].length, 1);
    assert.equal(submitted[11][0].visible, true);
    assert.equal(submitted[9].motion.vertices.length, 0);
    renderer.destroy();
    assert.ok(gpu.resources.every(resource => resource.destroyCount === 1));
  } finally {
    GpuTriangleRenderer.create = original;
  }
});
