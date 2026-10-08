// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Renderer backend identifiers accepted by the engine device factory.
 */
export const GPU_BACKEND = Object.freeze({
  WEBGPU: 'webgpu',
  WEBGL: 'webgl',
  BEST_AVAILABLE: 'best-available'
} as const);

/**
 * Texture formats shared by render targets and their consumers.
 */
export const GPU_TEXTURE_FORMAT = Object.freeze({
  RGBA16FLOAT: 'rgba16float',
  RGBA8UNORM: 'rgba8unorm',
  DEPTH24PLUS: 'depth24plus',
  DEPTH32FLOAT: 'depth32float',
  RG16FLOAT: 'rg16float',
  R32FLOAT: 'r32float'
} as const);

/**
 * Sampler minification and magnification filters.
 */
export const GPU_FILTER = Object.freeze({
  LINEAR: 'linear',
  NEAREST: 'nearest'
} as const);

/**
 * Texture address modes.
 */
export const GPU_ADDRESS_MODE = Object.freeze({
  CLAMP_TO_EDGE: 'clamp-to-edge',
  REPEAT: 'repeat',
  MIRROR_REPEAT: 'mirror-repeat'
} as const);

/**
 * Primitive topology for pipeline descriptors.
 */
export const GPU_TOPOLOGY = Object.freeze({
  TRIANGLE_LIST: 'triangle-list',
  LINE_LIST: 'line-list',
  TRIANGLE_STRIP: 'triangle-strip'
} as const);

/**
 * Vertex attribute formats.
 */
export const GPU_VERTEX_FORMAT = Object.freeze({
  FLOAT32X2: 'float32x2',
  FLOAT32X3: 'float32x3',
  FLOAT32X4: 'float32x4'
} as const);

/**
 * Depth comparison functions.
 */
export const GPU_COMPARE = Object.freeze({
  LESS_EQUAL: 'less-equal',
  LESS: 'less',
  ALWAYS: 'always'
} as const);

/**
 * Shared entry points in authored GPU programs.
 */
export const SHADER_ENTRY_POINT = Object.freeze({
  VERTEX: 'vertexMain',
  FRAGMENT: 'fragmentMain',
  COMPUTE: 'computeMain'
} as const);
