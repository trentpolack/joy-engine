// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { createPortableShaders } from '../src/rendering/gpu/gpu-device.ts';

test('portable GLSL compilation releases the vertex shader if fragment allocation fails', () => {
  const vertexShader = { destroyed: 0, destroy() { this.destroyed++; } };
  const device = {
    type: 'webgl',
    info: {
      type: 'webgl', vendor: 'test', renderer: 'unit-test-device', version: '1',
      gpu: 'software', gpuType: 'cpu', shadingLanguage: 'glsl', shadingLanguageVersion: 300
    },
    features: new Set(),
    limits: {},
    createShader(properties) {
      if(properties.stage === 'fragment') {
        throw new Error('fragment allocation failed');
      }
      return vertexShader;
    }
  };
  assert.throws(() => createPortableShaders(device, {
    wgsl: '', glsl: {
      vertex: '// Licensed shader\n\n#version 300 es\nvoid main() { gl_Position = vec4(0.0); }',
      fragment: '// Licensed shader\n\n#version 300 es\nprecision highp float;\nout vec4 color;\nvoid main() { color = vec4(1.0); }'
    }
  }), /fragment allocation failed/);
  assert.equal(vertexShader.destroyed, 1);
});
