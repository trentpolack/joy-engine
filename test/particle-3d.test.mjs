// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import * as engine from '../src/index.ts';

function effect(renderer = 'soft') {
  return new engine.ParticleEffect(engine.compileParticleEffect({
    version: 1, name: 'Spatial test', maxParticles: 8, parameters: {},
    emitters: [{ id: 'one', enabled: true, root: true, duration: 0, rate: 0,
      bursts: [{ time: 0, count: 1 }], lifetime: 2, renderer,
      spawn: 'size = 2; vx = 12; vy = 18; vz = 60; az = 6;', update: '' }],
  }), { position: { x: 10, y: 20, z: 30 } });
}

test('perspective orbit camera projects depth in both GPU clip conventions', () => {
  const camera = new engine.OrbitCamera();
  camera.yaw = 0;
  camera.pitch = 0;
  camera.distance = 100;
  for(const webgpu of [false, true]) {
    const m = camera.matrix(1, webgpu);
    const project = z => ({ x: m[0] * 10 / (m[11] * z + m[15]), z: (m[10] * z + m[14]) / (m[11] * z + m[15]) });
    assert.ok(project(50).x > project(0).x, 'near particles project larger');
    assert.ok(project(0).z > (webgpu ? 0 : -1) && project(0).z < 1);
  }
});

test('billboards follow camera axes without mutating XYZ simulation', () => {
  const runtime = effect();
  const before = JSON.stringify(runtime.particles);
  const camera = new engine.OrbitCamera();
  camera.yaw = Math.PI / 2;
  camera.pitch = 0;
  const vertices = [];
  engine.appendParticleEffect(vertices, runtime, camera.basis());
  const xs = [], zs = [];
  for(let i = 0; i < vertices.length; i += 7) {
    xs.push(vertices[i]);
    zs.push(vertices[i + 2]);
  }
  assert.ok(Math.max(...xs) - Math.min(...xs) < 1e-6, 'side view billboard lies in YZ');
  assert.ok(Math.max(...zs) - Math.min(...zs) > 3);
  assert.equal(JSON.stringify(runtime.particles), before);
  runtime.step();
  assert.ok(runtime.particles[0].z > 31 && runtime.particles[0].vz > 60);
});

test('streaks retain full world-space velocity depth', () => {
  const runtime = effect('streak');
  const camera = new engine.OrbitCamera();
  camera.yaw = 0;
  camera.pitch = 0;
  const vertices = [];
  engine.appendParticleEffect(vertices, runtime, camera.basis());
  const zs = vertices.filter((_, i) => i % 7 === 2);
  assert.ok(Math.min(...zs) <= 30 - 60 * .035, 'tail follows VZ even along view axis');
});

test('transparent ordering responds to camera direction', () => {
  const triangle = z => [0,0,z,1,1,1,.5, 1,0,z,1,1,1,.5, 0,1,z,1,1,1,.5];
  const vertices = [...triangle(10), ...triangle(-10)];
  assert.equal(engine.sortCameraTransparentTriangles(vertices, [0,0,1])[2], -10);
  assert.equal(engine.sortCameraTransparentTriangles(vertices, [0,0,-1])[2], 10);
  assert.equal(vertices[2], 10);
});
