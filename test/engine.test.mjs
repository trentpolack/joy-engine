// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import {
  addCircle,
  addQuad,
  addTriangle,
  DynamicGpuBuffer,
  GpuTriangleRenderer,
  addPostProcessingShaders,
  createPostProcessingUniforms,
  gpuBackendFromQuery,
  normalizePostProcessingConfig,
  toneMapperIndex,
  sortTopDownTransparentTriangles,
} from 'joy-engine';

test('top-down transparency sorting orders planes while retaining equal-depth authoring order', () => {
  const triangleAtDepth = (depth, marker) => [
    0, 0, depth, marker, 0, 0, 1,
    1, 0, depth, marker, 0, 0, 1,
    0, 1, depth, marker, 0, 0, 1,
  ];
  const vertices = [...triangleAtDepth(3, 1), ...triangleAtDepth(-2, 2), ...triangleAtDepth(3, 3)];
  const sorted = sortTopDownTransparentTriangles(vertices);

  assert.deepEqual([sorted[3], sorted[24], sorted[45]], [2, 1, 3]);
  assert.notEqual(sorted, vertices);
  assert.throws(() => sortTopDownTransparentTriangles([0]), /21 values/);
});

test('post-processing config produces portable shader code and packed frame uniforms', () => {
  const config = normalizePostProcessingConfig({ exposure: 1.4, filmGrain: 0.03 });
  assert.equal(config.exposure, 1.4);
  assert.equal(config.bloomStrength, 0);
  assert.equal(config.brightness, 1);
  assert.equal(config.toneMapper, 'agx');
  assert.equal(toneMapperIndex(config.toneMapper), 2);
  assert.equal(toneMapperIndex('aces'), 1);
  assert.equal(toneMapperIndex('none'), 0);
  assert.equal(gpuBackendFromQuery('?backend=webgpu'), 'webgpu');
  assert.equal(gpuBackendFromQuery('?backend=webgl'), 'webgl');
  assert.equal(gpuBackendFromQuery('?backend=core'), 'best-available');
  assert.throws(() => normalizePostProcessingConfig({ toneMapper: /** @type {any} */ ('reinhard') }), /toneMapper/);
  assert.throws(() => normalizePostProcessingConfig({ vignetteRadius: 2 }), /vignetteRadius/);
  assert.deepEqual(Array.from(createPostProcessingUniforms(config, 1920, 1080, 2.5)), [
    1920, 1080, 2.5, 0, 1.399999976158142, 0, 1, 0.029999999329447746,
    0.3499999940395355, 1, 1, 0.7200000286102295,
    0.02500000037252903, 0, 0, 1, 1.5, 18, 1, 0,
  ]);

  const sources = addPostProcessingShaders({
    wgsl: '// JOY_POST_UNIFORMS_MACRO\n// JOY_POST_DECLARATIONS_MACRO\n// JOY_POST_APPLY_MACRO',
    glsl: {
      vertex: '// JOY_POST_UNIFORMS_MACRO',
      fragment: '// JOY_POST_UNIFORMS_MACRO\n// JOY_POST_DECLARATIONS_MACRO\n// JOY_POST_APPLY_MACRO',
    },
  });
  assert.match(sources.wgsl, /return sourceColor/);
  assert.doesNotMatch(sources.glsl.fragment, /2\.51/);
  assert.match(sources.glsl.fragment, /fragmentColor = sourceColor/);
  assert.throws(() => normalizePostProcessingConfig({ bloomRadius: -1 }), /negative/);
  assert.equal(config.bloomRadius, 0.025);
  assert.throws(() => normalizePostProcessingConfig({ antialiasStrength: 2 }), /antialiasStrength/);
  assert.doesNotMatch(sources.glsl.vertex, /JOY_POST/);
});

test('dynamic GPU buffers grow geometrically, rebind, and enforce device limits', () => {
  const createdSizes = [];
  const replacements = [];
  const destroyedSizes = [];
  const device = {
    limits: { maxBufferSize: 128 },
    createBuffer({ byteLength }) {
      createdSizes.push(byteLength);
      return {
        byteLength,
        write() {},
        destroy() {
          destroyedSizes.push(byteLength);
        },
      };
    },
  };
  const buffer = new DynamicGpuBuffer(device, {
    usage: 1,
    initialByteLength: 16,
    onReplace: replacement => replacements.push(replacement),
  });
  buffer.write(new Uint8Array(17));
  buffer.write(new Uint8Array(65));

  assert.deepEqual(createdSizes, [16, 32, 128]);
  assert.equal(replacements.length, 3);
  assert.deepEqual(destroyedSizes, [16, 32]);
  assert.throws(() => buffer.write(new Uint8Array(129)), /device limit of 128/);
  buffer.destroy();
  assert.deepEqual(destroyedSizes, [16, 32, 128]);
});

test('geometry appends complete triangles with consistent area and vertex colors', () => {
  const color = [0.2, 0.4, 0.6, 0.8];
  for(const [append, count, expectedArea] of [
    [addTriangle, 3, 3 * Math.sqrt(3)],
    [addQuad, 6, 8],
    [addCircle, 36, 12],
  ]) {
    const vertices = [];
    append(vertices, 10, 20, 2, color);
    assert.equal(vertices.length, count * 7);
    let area = 0;
    for(let index = 0; index < vertices.length; index += 21) {
      const [ax, ay] = vertices.slice(index, index + 2);
      const [bx, by] = vertices.slice(index + 7, index + 9);
      const [cx, cy] = vertices.slice(index + 14, index + 16);
      area += ((bx - ax) * (cy - ay) - (by - ay) * (cx - ax)) / 2;
    }
    assert.ok(Math.abs(area - expectedArea) < 1e-10);
    for(let index = 0; index < vertices.length; index += 7) {
      assert.equal(vertices[index + 2], 0);
      assert.deepEqual(vertices.slice(index + 3, index + 7), color);
    }
    append(vertices, 10, 20, 2, color);
    assert.equal(vertices.length, count * 14);
  }
});

test('GPU submission clears empty frames and uploads populated batches', () => {
  const draws = [];
  const writes = [];
  const clears = [];
  const depthClears = [];
  const framebufferOptions = [];
  let submissions = 0;
  const pass = {
    setPipeline() {}, setBindings() {}, setVertexArray() {}, end() {},
    draw({ vertexCount }) {
      draws.push(vertexCount);
    },
  };
  const renderer = Object.create(GpuTriangleRenderer.prototype);
  Object.assign(renderer, {
    vertexBuffer: {
      byteLength: 24,
      write(data) {
        writes.push(data);
      },
      destroy() {},
    },
    uniformBuffer: { write(data) {
      writes.push(data);
    } },
    backend: 'webgl',
    vertexArray: { setBuffer() {} },
    pipeline: {},
    canvasContext: {
      getCurrentFramebuffer(options) {
        framebufferOptions.push(options);
        return {};
      },
    },
    device: {
      beginRenderPass({ clearColor, clearDepth }) {
        clears.push(clearColor);
        depthClears.push(clearDepth);
        return pass;
      },
      submit() {
        submissions += 1;
      },
    },
  });
  const uniforms = new Float32Array([800, 600, 0, 0]);
  const background = [0, 0, 0, 1];
  renderer.render([], uniforms, background);
  assert.equal(writes.length, 1);
  assert.equal(writes[0], uniforms);

  const vertices = [];
  addQuad(vertices, 0, 0, 2, [1, 1, 1, 1]);
  renderer.render(vertices, uniforms, background);
  assert.deepEqual(Array.from(writes[1]), vertices.map(Math.fround));
  assert.deepEqual(draws, [0, 6]);
  assert.deepEqual(clears, [background, background]);
  assert.equal(submissions, 2);
  assert.deepEqual(depthClears, [1, 1]);
    // Both pipelines and the framebuffer must agree on the depth format.
  assert.deepEqual(framebufferOptions, [
      { depthStencilFormat: 'depth24plus' },
      { depthStencilFormat: 'depth24plus' },
    ]);
});

test('opaque and translucent pipelines share depth but only opaque geometry writes it', () => {
  const pipelines = [];
  const device = {
    type: 'webgl', info: { shadingLanguage: 'glsl', gpu: 'test-device' },
    createShader() {
      return {};
    },
    createRenderPipeline(options) {
      pipelines.push(options);
      return { shaderLayout: {}, bufferLayout: options.bufferLayout };
    },
    createVertexArray() {
      return { setBuffer() {} };
    },
    createBuffer() {
      return {};
    },
  };
  new GpuTriangleRenderer(device, {}, { shaderSources: { glsl: { vertex: '', fragment: '' } } });
  assert.equal(pipelines.length, 2);
  assert.ok(pipelines.every(pipeline => pipeline.depthStencilAttachmentFormat === 'depth24plus'));
  assert.ok(pipelines.every(pipeline => pipeline.parameters.depthCompare === 'less-equal'));
  assert.equal(pipelines[0].parameters.depthWriteEnabled, true);
  assert.equal(pipelines[0].parameters.blend, false);
  assert.equal(pipelines[1].parameters.depthWriteEnabled, false);
  assert.equal(pipelines[1].parameters.blendColorOperation, 'add');
  assert.equal(pipelines[1].parameters.blendColorSrcFactor, 'src-alpha');
  assert.equal(pipelines[1].parameters.blendColorDstFactor, 'one-minus-src-alpha');
});

test('device creation retries WebGL when preferred WebGPU adapter acquisition fails', async () => {
  const { luma } = await import('@luma.gl/core');
  const originalCreateDevice = luma.createDevice;
  const attempts = [];
  const device = {
    type: 'webgl', info: { shadingLanguage: 'glsl', gpu: 'test-device' }, canvasContext: {},
    createShader() {
      return {};
    },
    createRenderPipeline(options) {
      return { shaderLayout: {}, bufferLayout: options.bufferLayout };
    },
    createVertexArray() {
      return { setBuffer() {} };
    },
    createBuffer() {
      return {};
    },
  };
  luma.createDevice = async options => {
    attempts.push(options.type);
    if(options.type === 'best-available') {
      throw new Error('Failed to request WebGPU adapter');
    }
    return device;
  };
  try {
    const renderer = await GpuTriangleRenderer.create({}, { shaderSources: { glsl: { vertex: '', fragment: '' } } });
    assert.equal(renderer.backend, 'webgl');
    assert.deepEqual(attempts, ['best-available', 'webgl']);
  } finally {
    luma.createDevice = originalCreateDevice;
  }
});
