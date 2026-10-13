// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {decodeTerrain, MeshAsset, OrbitCamera, RenderScene, SceneRenderer} from 'joy-engine';
import type {GpuBackend} from 'joy-engine';

/** Browser fixture for one hierarchy containing CPU meshes and resident indexed terrain. */
export async function createSceneGraphFixture(canvas: HTMLCanvasElement, backend: GpuBackend) {
  const scene = new RenderScene();
  const camera = new OrbitCamera();
  camera.target = [0, 0.7, 0];
  camera.radius = 9;
  camera.distance = 22;
  camera.pitch = 0.8;
  camera.yaw = 0.4;
  camera.nearClip = 0.1;
  camera.farClip = 150;
  const renderer = new SceneRenderer(canvas, {camera, pixelRatio: 1,
    device: {backend, fallbackToWebGl: false}, shadows: {directionalMapSize: 512, pointMapSize: 512},
    postProcessing: {bloomStrength: 0, antialiasStrength: 0}});
  await renderer.initialize();
  const heights = new Uint16Array(17*17);
  const terrainAsset = decodeTerrain({version: 1, size: 17, tileCells: 8, extent: [18, 18],
    heightOffset: 0, heightScale: 1, heightFile: 'fixture.bin', orientation: 'row-0-to-negative-z',
    patches: [{x:0,z:0}, {x:8,z:0}, {x:0,z:8}, {x:8,z:8}].map(patch => ({...patch, min:0, max:0, errors:[0,0,0,0]}))}, heights.buffer);
  const world = scene.createNode({position: [0.5, 0, 0], rotation: [0, 0.1, 0]});
  const terrain = scene.createTerrain(terrainAsset, {parent: world});
  const rig = scene.createNode({parent: world, position: [-1.5, 0, 0], scale: [1.6, 1, 0.75]});
  const cube = new MeshAsset({positions:[-1,-1,-1,1,-1,-1,1,1,-1,-1,1,-1,-1,-1,1,1,-1,1,1,1,1,-1,1,1],
    indices:[0,2,1,0,3,2,4,5,6,4,6,7,0,1,5,0,5,4,3,7,6,3,6,2,0,4,7,0,7,3,1,2,6,1,6,5]});
  const mesh = scene.createMesh(cube, {parent: rig, position: [0, 2, 0], rotation: [0, 0.6, 0], tint: [0.95, 0.3, 0.06]});
  // A second native surface can occlude the mesh, proving terrain participates as a caster too.
  const canopy = scene.createTerrain(terrainAsset, {parent: world, position:[-1,4,-2], scale:[0.2,1,0.2], visible:false});
  const lights = [{x:-4,y:7,z:-4,r:3,g:3,b:3,range:30}];
  function render() {
    renderer.render(scene, {lights, ambient: 0.08});
    return {diagnostics: renderer.getDiagnostics(), transform: mesh.removed ? [] : Array.from(mesh.worldTransform)};
  }
  return {scene, renderer, terrain, canopy, world, rig, mesh, camera, render,
    destroy() { renderer.destroy(); scene.destroy(); }};
}
