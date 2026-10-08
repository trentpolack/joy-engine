// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import { SceneEffects, normalizeSceneEffectsOptions, getSceneEffectsRequirements } from '../src/rendering/scene/scene-effects.ts';
import { createSceneEffectPipelines, validateSceneEffectFormats } from '../src/rendering/scene/scene-effect-pipelines.ts';

const EFFECT_NAMES = ['depthAwareBlur', 'ssao', 'gtao', 'ssgi', 'outlines', 'taa', 'motionBlur', 'ssr', 'heightFog', 'clusteredLighting', 'adaptiveExposure'];
const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

function createHarness(options) {
  const device = { type: 'webgl' };
  const effects = new SceneEffects(device, options);
  effects.configuration = normalizeSceneEffectsOptions({ colorFormat: 'rgba16float', ...options });
  const calls = [];
  const source = { device, width: 32, height: 24, format: 'rgba16float', destroy() { throw new Error('Borrowed textures must remain alive.'); } };
  const texture = format => ({ ...source, format });
  const stages = createSceneEffectPipelines(effects.configuration);
  for(const [group, property] of [['lighting', 'renderer'], ['temporal', 'temporalRenderer'], ['finishing', 'finishingRenderer']]) {
    if(!stages[group].length) {
      continue;
    }
    effects[property] = {
      resize(size) { calls.push({ group, operation: 'resize', size }); },
      resetHistory() { calls.push({ group, operation: 'reset' }); },
      destroy() { calls.push({ group, operation: 'destroy' }); },
      renderToTexture(frame) { calls.push({ group, operation: 'render', frame }); return { ...source, group }; }
    };
  }
  const frame = {
    depthTexture: texture('depth24plus'), normalTexture: texture('rgba16float'),
    velocityTexture: texture('rg16float'), taaVelocityTexture: texture('rg16float'),
    projectionMatrix: identity, inverseProjectionMatrix: identity, inverseViewMatrix: identity,
    inverseViewProjectionMatrix: identity, previousViewProjectionMatrix: identity,
    inverseRasterViewProjectionMatrix: identity, previousRasterViewProjectionMatrix: identity,
    nearPlane: 0.5, farPlane: 100, currentJitter: [0.001, -0.001], previousJitter: [-0.001, 0.001],
    timeSeconds: 12, deltaTimeSeconds: 1/60
  };
  return { effects, device, source, frame, calls };
}

test('WebGL reports every requested advanced effect and preserves borrowed source resources', () => {
  const requested = Object.fromEntries(EFFECT_NAMES.filter(name => name !== 'gtao').map(name => [name, true]));
  const effects = new SceneEffects({ type: 'webgl' }, requested);
  assert.equal(effects.capabilities.backend, 'webgl');
  assert.equal(effects.capabilities.available, false);
  for(const name of EFFECT_NAMES) {
    assert.deepEqual(effects.capabilities.effects[name], {
      requested: name !== 'gtao', available: false, reason: name !== 'gtao' ? 'Requires the WebGPU backend.' : null
    });
  }
  const source = { width: 32, height: 24 };
  assert.equal(effects.render(source), source);
  effects.resize(64, 48);
  effects.resetHistory();
  effects.destroy();
  effects.destroy();
  assert.throws(() => effects.render(source), /destroyed/);
});

test('all effect tuning and unsupported combinations fail before GPU allocation', () => {
  const device = { type: 'webgpu', getTextureFormatCapabilities() { throw new Error('GPU capability query must not occur.'); } };
  for(const name of EFFECT_NAMES) {
    assert.throws(() => new SceneEffects(device, { [name]: { unsupported: true } }), /unknown property/);
    assert.throws(() => new SceneEffects(device, { [name]: [] }), /object/);
  }
  for(const [name, settings, field] of [
    ['ssao', { radius: 0 }, 'radius'], ['ssao', { intensity: 1e100 }, 'intensity'], ['depthAwareBlur', { radius: 9 }, 'radius'],
    ['gtao', { historyWeight: 1 }, 'historyWeight'], ['ssgi', { rayCount: 13 }, 'rayCount'],
    ['outlines', { color: [1, 1, NaN, 1] }, 'color'], ['taa', { reprojection: 'other' }, 'reprojection'],
    ['motionBlur', { sampleCount: 17 }, 'sampleCount'], ['ssr', { sampleCount: 8.5 }, 'integer'],
    ['heightFog', { density: -1 }, 'density'], ['clusteredLighting', { sampleCount: 21 }, 'sampleCount'],
    ['adaptiveExposure', { minimumExposure: 4, maximumExposure: 2 }, 'minimumExposure']
  ]) {
    assert.throws(() => new SceneEffects(device, { [name]: settings }), new RegExp(field));
  }
  assert.throws(() => new SceneEffects(device, { ssao: true, gtao: true }), /mutually exclusive/);
  assert.throws(() => new SceneEffects(device, { colorFormat: 'bgra8unorm' }), /colorFormat/);
  assert.throws(() => new SceneEffects(device, { unknown: true }), /unknown/);
});

test('disabled effects skip GPU resources and explicit unavailable HDR fails before allocation', () => {
  const device = { type: 'webgpu', getTextureFormatCapabilities() { return { create: true, render: true, filter: false, blend: true }; } };
  const effects = new SceneEffects(device);
  assert.equal(effects.capabilities.available, true);
  const source = {};
  assert.equal(effects.render(source), source);
  effects.destroy();
  assert.throws(() => new SceneEffects(device, { colorFormat: 'rgba16float', taa: true }), /not supported/);
});

test('pipeline color histories retain HDR and temporal depth histories retain full hardware precision', () => {
  const pipelines = createSceneEffectPipelines(normalizeSceneEffectsOptions({ colorFormat: 'rgba16float', gtao: true, ssgi: true, ssr: true, taa: true, heightFog: true, adaptiveExposure: true }));
  const targets = Object.assign({}, ...Object.values(pipelines).flatMap(group => group.map(pipeline => pipeline.renderTargets)));
  for(const name of ['gtaoHistoryDepth', 'ssgiHistoryDepth', 'ssrHistoryDepth', 'taaHistoryDepth']) {
    assert.equal(targets[name].format, 'rgba32float');
    assert.equal(targets[name].sampler.minFilter, 'nearest');
  }
  for(const name of ['ssgiHistory', 'ssrHistory', 'taaHistoryColor', 'fogHistory']) {
    assert.equal(targets[name].format, 'rgba16float');
    assert.equal(targets[name].sampler.minFilter, 'linear');
  }
  assert.throws(() => validateSceneEffectFormats({ getTextureFormatCapabilities(format) { return { create: true, render: true, filter: format !== 'rgba32float' }; } }, pipelines), /float32-filterable/);
});

test('frame contract rejects malformed borrowed inputs before target resize or history mutation', () => {
  const { effects, source, frame, calls } = createHarness({ ssao: true, taa: true });
  assert.throws(() => effects.render(source, { ...frame, nearPlane: 0 }), /nearPlane/);
  assert.throws(() => effects.render(source, { ...frame, projection: 'orthographic' }), /perspective/);
  assert.throws(() => effects.render(source, { ...frame, depthTexture: { ...frame.depthTexture, width: 1 } }), /dimensions/);
  assert.throws(() => effects.render(source, { ...frame, taaVelocityTexture: { ...frame.taaVelocityTexture, device: {} } }), /device/);
  assert.throws(() => effects.render(source, { ...frame, taaVelocityTexture: undefined }), /live GPU/);
  assert.throws(() => effects.render(source, { ...frame, currentJitter: [0, NaN] }), /finite/);
  assert.throws(() => effects.render(source, { ...frame, resetHistory: 1 }), /boolean/);
  assert.equal(calls.length, 0);
  effects.destroy();
});

test('lighting, TAA and finishing use correct motion spaces and invalidate their independent histories', () => {
  const { effects, source, frame, calls } = createHarness({ gtao: true, ssgi: true, ssr: true, taa: true, motionBlur: true, adaptiveExposure: true, heightFog: true });
  assert.equal(effects.render(source, frame).group, 'finishing');
  const renders = calls.filter(call => call.operation === 'render');
  assert.deepEqual(renders.map(call => call.group), ['lighting', 'temporal', 'finishing']);
  assert.equal(renders[0].frame.bindings.velocityTexture, frame.velocityTexture);
  assert.equal(renders[1].frame.bindings.velocityTexture, frame.taaVelocityTexture);
  assert.equal(renders[2].frame.bindings.velocityTexture, frame.taaVelocityTexture);
  assert.equal(renders[1].frame.sourceTexture.group, 'lighting');
  assert.equal(renders[2].frame.sourceTexture.group, 'temporal');
  assert.deepEqual(renders[1].frame.uniforms.taaResolve.currentJitter, frame.currentJitter);
  assert.equal(renders[0].frame.uniforms.volumetricFog.historyWeight, 0);
  assert.equal(renders[2].frame.uniforms.hdrAutoExposureAdapt.deltaTime, frame.deltaTimeSeconds);
  calls.length = 0;
  effects.render(source, { ...frame, nearPlane: 1 });
  assert.equal(calls.filter(call => call.operation === 'reset').length, 3);
  calls.length = 0;
  effects.destroy();
  effects.destroy();
  assert.deepEqual(calls.map(call => call.group), ['finishing', 'temporal', 'lighting']);
});

test('camera reprojection requires current inverse and previous matrices without a velocity attachment', () => {
  const { effects, source, frame, calls } = createHarness({ taa: { reprojection: 'camera' } });
  assert.equal(getSceneEffectsRequirements(effects.configuration).taaVelocityTexture, false);
  assert.throws(() => effects.render(source, { ...frame, inverseViewProjectionMatrix: [1] }), /16-number matrix/);
  assert.equal(calls.length, 0);
  effects.render(source, { ...frame, velocityTexture: undefined, taaVelocityTexture: undefined });
  const options = calls.find(call => call.operation === 'render').frame;
  assert.equal(options.uniforms.cameraReprojectionTaaResolve.previousViewProjectionMatrix, identity);
  effects.destroy();
});

test('partial frame failure invalidates every previously committed history', () => {
  const { effects, source, frame, calls } = createHarness({ gtao: true, taa: true });
  effects.temporalRenderer.renderToTexture = () => { throw new Error('GPU frame failure'); };
  assert.throws(() => effects.render(source, frame), /GPU frame failure/);
  assert.equal(calls.filter(call => call.operation === 'reset').length, 4);
  assert.equal(effects.frameIndex, 0);
  assert.equal(effects.depthConvention, null);
  effects.destroy();
});

test('clustered lighting bounds and buffer sizes are validated before any render history changes', () => {
  const { effects, device, source, frame, calls } = createHarness({ clusteredLighting: true });
  const buffer = byteLength => ({ device, byteLength });
  const clusters = { pointLights: buffer(32), clusterLightCounts: buffer(4), clusterLightIndices: buffer(32), clusterCountX: 1, clusterCountY: 1, clusterCountZ: 1, maxLightsPerCluster: 8, pointLightCount: 0 };
  assert.throws(() => effects.render(source, { ...frame, clusteredLighting: { ...clusters, pointLightCount: 33 } }), /pointLightCount/);
  assert.throws(() => effects.render(source, { ...frame, clusteredLighting: { ...clusters, clusterLightIndices: buffer(4) } }), /at least 32 bytes/);
  assert.equal(calls.length, 0);
  effects.render(source, { ...frame, clusteredLighting: clusters });
  const rendered = calls.find(call => call.operation === 'render').frame;
  assert.equal(rendered.bindings.pointLights, clusters.pointLights);
  assert.equal(rendered.uniforms.clusteredVolumetricTrace.pointLightCount, 0);
  effects.destroy();
});
