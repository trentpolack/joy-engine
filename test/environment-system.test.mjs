import test from 'node:test';
import assert from 'node:assert/strict';
import {
  EnvironmentSystem,
  ATMOSPHERE_RENDER_UNIFORM_BYTES,
  ATMOSPHERE_SHADERS,
  ENVIRONMENT_UNIFORM_BYTES,
  createAtmosphereRenderUniforms,
  createEnvironmentLighting,
  createEnvironmentUniforms
} from 'joy-engine';

test('environment simulation advances a deterministic solar clock and wind field', () => {
  const first = new EnvironmentSystem({timeOfDay: 0.25, dayLengthSeconds: 100, timeScale: 2});
  const second = new EnvironmentSystem({timeOfDay: 0.25, dayLengthSeconds: 100, timeScale: 2});
  const firstFrame = first.step(10);
  const secondFrame = second.step(10);

  assert.ok(Math.abs(firstFrame.timeOfDay - 0.45) < Number.EPSILON);
  assert.deepEqual(firstFrame, secondFrame);
  assert.equal(firstFrame.primaryLight.kind, 'sun');
  assert.equal(Object.isFrozen(firstFrame), true);
  assert.equal(Object.isFrozen(firstFrame.wind.velocity), true);
});

test('environment packs portable atmosphere rendering state', () => {
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const frame = new EnvironmentSystem({timeOfDay: 0.5}).sample();
  const uniforms = createAtmosphereRenderUniforms(identity, [4, 8, 12], [1920, 1080], frame);

  assert.equal(uniforms.byteLength, ATMOSPHERE_RENDER_UNIFORM_BYTES);
  assert.deepEqual(Array.from(uniforms.slice(16, 19)), [4, 8, 12]);
  assert.ok(Array.from(uniforms.slice(24, 27)).every((value, index) => Math.abs(value - frame.sun.direction[index]) < 1e-6));
  assert.match(ATMOSPHERE_SHADERS.wgsl, /cloudNoise/);
  assert.match(ATMOSPHERE_SHADERS.glsl.fragment, /rayleighPhase/);
  assert.throws(() => createAtmosphereRenderUniforms(identity, [0, 0, 0], [0, 1080], frame), /viewport/);
});

test('environment wraps time, supports a paused day, and selects the moon at night', () => {
  const environment = new EnvironmentSystem({timeOfDay: 0.95, dayLengthSeconds: 10});
  assert.equal(environment.step(1).timeOfDay, 0.050000000000000044);
  assert.equal(environment.sample().primaryLight.kind, 'moon');

  const paused = new EnvironmentSystem({timeOfDay: 0.5, dayLengthSeconds: 0});
  paused.step(60);
  assert.equal(paused.sample().timeOfDay, 0.5);
  assert.equal(paused.sample().elapsedSeconds, 60);
});

test('environment produces aligned render data and PBR lighting', () => {
  const frame = new EnvironmentSystem({
    timeOfDay: 0.5,
    atmosphere: {fogDensity: 0.01},
    clouds: {coverage: 0.8},
    lightShafts: {intensity: 2}
  }).sample();
  const uniforms = createEnvironmentUniforms(frame);
  const lighting = createEnvironmentLighting(frame);

  assert.equal(uniforms.byteLength, ENVIRONMENT_UNIFORM_BYTES);
  assert.ok(Math.abs(uniforms[28] - 0.01) < 1e-6);
  assert.ok(Math.abs(uniforms[29] - 0.8) < 1e-6);
  assert.equal(uniforms[34], 2);
  assert.deepEqual(lighting.lightDirection, frame.primaryLight.direction);
  assert.ok(lighting.lightColor.every(Number.isFinite));
  assert.ok(lighting.ambientColor.every(value => value > 0));
});

test('environment rejects invalid policy before state can advance', () => {
  assert.throws(() => new EnvironmentSystem({timeOfDay: 1}), /less than 1|between/);
  assert.throws(() => new EnvironmentSystem({clouds: {coverage: 2}}), /coverage/);
  assert.throws(() => new EnvironmentSystem({wind: {gustPeriodSeconds: 0}}), /gustPeriodSeconds/);
  assert.throws(() => new EnvironmentSystem(/** @type {any} */ ({weather: 'rain'})), /weather/);
  const environment = new EnvironmentSystem();
  assert.throws(() => environment.step(-1), /deltaSeconds/);
  assert.throws(() => environment.setTimeOfDay(1), /less than 1/);
});


test('atmosphere resources roll back on startup failure and are released once', async t => {
  const { luma } = await import('@luma.gl/core');
  const { GpuTriangleRenderer } = await import('joy-engine');
  const resources = [];
  const resource = extra => {
    const owned = {destroyed: 0, destroy() { this.destroyed++; }, ...extra};
    resources.push(owned);
    return owned;
  };
  let fail = true;
  const device = {
    type: 'webgpu', info: {shadingLanguage: 'wgsl', gpu: 'test'}, features: new Set(), limits: {},
    canvasContext: resource({}), destroy() {},
    createShader({id}) {
      if(fail && id === 'atmosphere') {
        throw new Error('atmosphere shader unavailable');
      }
      return resource({});
    },
    createRenderPipeline(options) { return resource({shaderLayout: {}, bufferLayout: options.bufferLayout}); },
    createVertexArray() { return resource({setBuffer() {}}); },
    createBuffer(options) { return resource({byteLength: options.byteLength}); }
  };
  t.mock.method(luma, 'createDevice', async () => device);
  const options = {atmosphere: true, shaderSources: {wgsl: '', glsl: {vertex: '', fragment: ''}}};
  await assert.rejects(GpuTriangleRenderer.create({}, options), /atmosphere shader unavailable/);
  assert.ok(resources.every(owned => owned.destroyed === 1), 'failed atmosphere startup releases core scene resources');
  resources.length = 0;
  fail = false;
  device.canvasContext = resource({});
  const renderer = await GpuTriangleRenderer.create({}, options);
  renderer.destroy();
  renderer.destroy();
  assert.ok(resources.every(owned => owned.destroyed === 1), 'each resource is disposed exactly once');
});
