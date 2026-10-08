import assert from 'node:assert/strict';
import test from 'node:test';
import * as profiles from '../src/rendering/postprocessor/profile.ts';
import { PostProcessingSession } from '../src/rendering/postprocessor/session.ts';
import { createEffectPipeline } from '../src/rendering/postprocessor/effect-pipeline.ts';
import { Postprocessor } from '../src/rendering/postprocessor/postprocessor.ts';
import { GpuTriangleRenderer } from 'joy-engine';

const profile = (overrides = {}, preset = 'default') => ({ version: 1, preset, overrides });

test('profiles resolve defaults, preset, and overrides without sharing mutable data', () => {
  const authored = profile({ exposure: 2 }, 'soft');
  const resolved = profiles.resolvePostProcessingProfile(authored);
  assert.equal(resolved.exposure, 2);
  assert.equal(resolved.bloomStrength, 0.25);
  resolved.exposure = 9;
  assert.equal(authored.overrides.exposure, 2);
  assert.equal(profiles.resolvePostProcessingProfile(authored).exposure, 2);
  const legacyGrain = profile({ filmGrainNoise: 'blue-noise' });
  assert.equal(profiles.resolvePostProcessingProfile(legacyGrain).filmGrainNoise, 'decorrelated');
  assert.equal(profiles.validatePostProcessingProfile(legacyGrain).overrides.filmGrainNoise, 'decorrelated');
  assert.equal(legacyGrain.overrides.filmGrainNoise, 'blue-noise');
});

test('profile boundary rejects invalid versions, unknown fields, and unsafe numeric values', () => {
  for(const value of [null, [], { ...profile(), version: 2 }, profile({}, 'missing'),
    profile({ exposure: -1 }), profile({ exposure: Infinity }), profile({ bloomKnee: 0 }),
    profile({ antialiasStrength: 2 }), profile({ exposure: '2' }), profile({ typo: 1 }),
    { ...profile(), extra: true }, { ...profile(), overrides: [] }]) {
    assert.throws(() => profiles.validatePostProcessingProfile(value));
  }
});

test('editor reset removes inheritance overrides; undo, redo and revert preserve saved values', () => {
  const session = new PostProcessingSession(profile({ exposure: 2 }), 'a');
  session.setValue('exposure', 3);
  assert.equal(session.dirty, true);
  session.resetValue('exposure');
  assert.equal(session.config.exposure, 1);
  session.undo();
  assert.equal(session.config.exposure, 3);
  session.redo();
  assert.equal(session.config.exposure, 1);
  session.revert();
  assert.equal(session.config.exposure, 2);
  assert.equal(session.dirty, false);
});

test('invalid edits are atomic and clean disk updates apply immediately', () => {
  const session = new PostProcessingSession(profile(), 'a');
  assert.throws(() => session.setValue('exposure', -1));
  assert.equal(session.config.exposure, 1);
  assert.equal(session.canUndo, false);
  session.receiveExternal(profile({ exposure: 4 }), 'b');
  assert.equal(session.config.exposure, 4);
  assert.equal(session.revision, 'b');
  assert.equal(session.dirty, false);
});

test('dirty external conflicts require an explicit local or disk choice', () => {
  const session = new PostProcessingSession(profile(), 'a');
  session.setValue('exposure', 3);
  session.receiveExternal(profile({ exposure: 4 }), 'b');
  assert.equal(session.config.exposure, 3);
  assert.ok(session.conflict);
  session.keepLocal();
  assert.equal(session.revision, 'b');
  assert.equal(session.dirty, true);
  session.receiveExternal(profile({ exposure: 5 }), 'c');
  session.useDisk();
  assert.equal(session.config.exposure, 5);
  assert.equal(session.dirty, false);
});

test('save completion preserves edits made while saving and clears own watch echoes', () => {
  const session = new PostProcessingSession(profile(), 'a');
  session.setValue('exposure', 2);
  const submitted = session.draft;
  session.setValue('exposure', 3);
  session.receiveExternal(submitted, 'b');
  session.markSaved(submitted, 'b');
  assert.equal(session.config.exposure, 3);
  assert.equal(session.saved.overrides.exposure, 2);
  assert.equal(session.dirty, true);
  assert.equal(session.conflict, null);
});

test('renderer updates are validated without replacing the pipeline or targets', () => {
  const renderer = Object.create(GpuTriangleRenderer.prototype);
  renderer.postProcessing = profiles.resolvePostProcessingProfile(profile());
  renderer.postprocessor = { targets: ['existing target'] };
  const resources = renderer.postprocessor;
  renderer.setPostProcessingConfig({ exposure: 2 });
  assert.equal(renderer.postProcessing.exposure, 2);
  assert.equal(renderer.postprocessor, resources);
  assert.throws(() => renderer.setPostProcessingConfig({ exposure: -1 }));
  assert.equal(renderer.postProcessing.exposure, 2);
});

test('renderer rejects a scene-effects format that differs from its scene target', () => {
  const device = {
    getTextureFormatCapabilities() {
      return { create: true, render: true, filter: true, blend: true };
    }
  };
  assert.throws(() => new GpuTriangleRenderer(device, {}, {
    shaderSources: { wgsl: '', glsl: { vertex: '', fragment: '' } },
    postProcessing: {},
    sceneEffects: { colorFormat: 'rgba8unorm', taa: true }
  }), /must match the post-processing scene format rgba16float/);
});

test('vignette blur pipeline gives its named filter targets linear sampling', () => {
  const config = profiles.resolvePostProcessingProfile(profile({ vignetteBlur: 0.5 }));
  const pipeline = createEffectPipeline(config, 720, 'rgba16float')
    .find(pass => pass.name === 'vignetteBlurPipeline');
  assert.ok(pipeline && 'renderTargets' in pipeline);
  for(const target of Object.values(pipeline.renderTargets ?? {})) {
    assert.equal(target.sampler?.minFilter, 'linear');
    assert.equal(target.sampler?.magFilter, 'linear');
    assert.equal(target.sampler?.addressModeU, 'clamp-to-edge');
    assert.equal(target.sampler?.addressModeV, 'clamp-to-edge');
  }
});

test('failed scene-target resize retains the working target and releases partial allocation', () => {
  let partialTextureDestroyed = 0;
  const device = {
    getTextureFormatCapabilities() {
      return { create: true, render: true, filter: true, blend: true };
    },
    createTexture({ id }) {
      if(id === 'postprocessor-scene-depth') {
        throw new Error('depth allocation failed');
      }
      return { destroy() { partialTextureDestroyed+= 1; } };
    }
  };
  const postprocessor = new Postprocessor(device);
  const destroyed = { framebuffer: 0, depth: 0, texture: 0 };
  const existing = {
    framebuffer: { destroy() { destroyed.framebuffer+= 1; } },
    depth: { destroy() { destroyed.depth+= 1; } },
    texture: { destroy() { destroyed.texture+= 1; } }
  };
  postprocessor.targets = [existing];
  postprocessor.width = 320;
  postprocessor.height = 180;

  assert.throws(() => postprocessor.resize(640, 360, 640, 360), /depth allocation failed/);
  assert.equal(postprocessor.targets[0], existing);
  assert.deepEqual(destroyed, { framebuffer: 0, depth: 0, texture: 0 });
  assert.equal(partialTextureDestroyed, 1);
});

test('reverting while a save is pending survives its watcher echo', () => {
  const session = new PostProcessingSession(profile({ exposure: 1 }), 'a');
  session.setValue('exposure', 2);
  const submitted = session.beginSave();
  session.revert();
  session.receiveExternal(submitted, 'b');
  session.markSaved(submitted, 'b');
  assert.equal(session.config.exposure, 1);
  assert.equal(session.saved.overrides.exposure, 2);
  assert.equal(session.dirty, true);
});

test('restoring the original disk revision clears an obsolete conflict', () => {
  const original = profile({ exposure: 1 });
  const session = new PostProcessingSession(original, 'a');
  session.setValue('exposure', 3);
  session.receiveExternal(profile({ exposure: 2 }), 'b');
  session.receiveExternal(original, 'a');
  assert.equal(session.conflict, null);
  assert.equal(session.config.exposure, 3);
  assert.equal(session.revision, 'a');
});
