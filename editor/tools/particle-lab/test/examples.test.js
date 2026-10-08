// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import {compileParticleEffect} from 'joy-engine';
import {EXAMPLES, createEmitterTemplate, EMITTER_PRESETS} from '../src/examples.ts';

test('bundled examples compile with current aggregate scripting syntax', () => {
  for(const example of EXAMPLES) {
    const compiled = compileParticleEffect(example.asset);
    assert.equal(compiled.emitters.length, example.asset.emitters.length, example.label);
  }

  const template = createEmitterTemplate('starter');
  assert.doesNotThrow(() => compileParticleEffect({
    version: 1,
    name: 'Starter',
    maxParticles: 128,
    parameters: {},
    emitters: [template]
  }));
});

test('emitter presets compile without shared parameters or child references and are detached', () => {
  for(const preset of EMITTER_PRESETS) {
    const emitter = createEmitterTemplate('new_emitter', preset.id);
    const asset = {version: 1, name: 'Preset', maxParticles: 512, parameters: {}, emitters: [emitter]};
    assert.doesNotThrow(() => compileParticleEffect(asset), preset.label);
    assert.equal(emitter.id, 'new_emitter');
    assert.equal(emitter.trail, undefined);
    assert.equal(emitter.death, undefined);
    emitter.bursts.push({time: 1, count: 1});
    assert.notDeepEqual(createEmitterTemplate('new_emitter', preset.id).bursts, emitter.bursts);
  }
});
