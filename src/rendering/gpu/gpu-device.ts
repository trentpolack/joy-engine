// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_BACKEND } from './constants.ts';
import type { CreateDeviceProps, Device } from '@luma.gl/core';
import type { ShaderModule, PlatformInfo } from '@luma.gl/shadertools';
import { luma } from '@luma.gl/core';
import { webgl2Adapter } from '@luma.gl/webgl';
import { webgpuAdapter } from '@luma.gl/webgpu';
import { GLSLShaderAssembler, WGSLShaderAssembler } from '@luma.gl/shadertools';
import shaderColorGlsl from '../../../shaders/common/color.glsl?raw';
import shaderColorWgsl from '../../../shaders/common/color.wgsl?raw';
import shaderConstantsGlsl from '../../../shaders/common/constants.glsl?raw';
import shaderConstantsWgsl from '../../../shaders/common/constants.wgsl?raw';

const SHADER_COLOR_MODULE = {
  name: 'joy-engine-shader-color',
  source: shaderColorWgsl,
  vs: shaderColorGlsl,
  fs: shaderColorGlsl
};

const SHADER_CONSTANTS_MODULE = {
  name: 'joy-engine-shader-constants',
  source: shaderConstantsWgsl,
  vs: shaderConstantsGlsl,
  fs: shaderConstantsGlsl
};

export type GpuBackend = typeof GPU_BACKEND.BEST_AVAILABLE | typeof GPU_BACKEND.WEBGPU | typeof GPU_BACKEND.WEBGL;
export interface GpuDeviceOptions {
  backend?: GpuBackend;
  optionalFeatures?: CreateDeviceProps['optionalFeatures'];
  fallbackToWebGl?: boolean;
  id?: string;
  debug?: boolean;
  powerPreference?: 'high-performance' | 'low-power';
  onError?: (error: Error, context: unknown) => boolean;
}
export interface PortableShaderSources {
  wgsl: string;
  glsl: {
      vertex: string;
      fragment: string;
  };
  modules?: ShaderModule[];
}

/**
 * Create the repository's portable luma.gl device and its default canvas context.
 *
 * Backend registration stays explicit so bundlers include only the adapters the
 * engine supports. A WebGL retry handles browsers that expose WebGPU while
 * failing adapter acquisition; callers can disable it when WebGPU is required.
 *
 * @param canvas
 * @param [options]
 * @returns Caller-owned device with a canvas context; destroy after dependent resources.
 */
export async function createGpuCanvasDevice(canvas: HTMLCanvasElement | OffscreenCanvas, options: GpuDeviceOptions = {}): Promise<Device> {
  const {
    backend = GPU_BACKEND.BEST_AVAILABLE,
    optionalFeatures,
    fallbackToWebGl = backend === GPU_BACKEND.BEST_AVAILABLE,
    id,
    debug,
    powerPreference,
    onError,
  } = options;

  const deviceOptions: CreateDeviceProps = {
    adapters: [webgpuAdapter, webgl2Adapter],
    waitForPageLoad: false,
    ...(optionalFeatures ? {optionalFeatures} : {}),
    createCanvasContext: { canvas, autoResize: false, alphaMode: 'premultiplied' },
    ...(id === undefined ? {} : { id }),
    ...(debug === undefined ? {} : { debug }),
    ...(powerPreference === undefined ? {} : { powerPreference }),
    ...(onError === undefined ? {} : { onError }),
  };

  let device;
  try {
    device = await luma.createDevice({ ...deviceOptions, type: backend });
  } catch (preferredError) {
    if(!fallbackToWebGl || backend === GPU_BACKEND.WEBGL) {
      throw preferredError;
    }
    try {
      device = await luma.createDevice({ ...deviceOptions, type: GPU_BACKEND.WEBGL });
    } catch (fallbackError) {
      throw new Error(
        `GPU device creation failed. Preferred backend: ${String(preferredError)}. WebGL 2 fallback: ${String(fallbackError)}`,
        { cause: fallbackError },
      );
    }
  }

  if(!device.canvasContext) {
    device.destroy();
    throw new Error('The selected GPU device did not create a canvas context.');
  }
  return device;
}

/**
 * Resolve an explicit development backend from a URL query. Unknown or absent
 * values retain luma.gl's WebGPU-first selection and WebGL 2 fallback.
 * @param search
 */
export function gpuBackendFromQuery(search: string): GpuBackend {
  const backend = new URLSearchParams(search).get('backend');
  if(backend === GPU_BACKEND.WEBGPU || backend === GPU_BACKEND.WEBGL) {
    return backend;
  }
  return GPU_BACKEND.BEST_AVAILABLE;
}

/**
 * Assemble modules and compile the shader language selected by the device.
 * Each compilation owns an isolated assembler; registering modules cannot
 * change shaders in another game or renderer. Callers own returned shaders.
 * @param device
 * @param sources
 * @param [id]
 */
export function createPortableShaders(device: Device, sources: PortableShaderSources, id: string = 'portable-shader') {
  const modules = [SHADER_COLOR_MODULE, SHADER_CONSTANTS_MODULE, ...(sources.modules ?? [])];

  const platformInfo: PlatformInfo = {
    type: device.type,
    shaderLanguage: device.info.shadingLanguage,
    shaderLanguageVersion: 300,
    gpu: device.info.gpu,
    features: new Set(device.features),
    limits:  ((((device.limits) as unknown)) as Record<string, number>)
  };

  if(device.info.shadingLanguage === 'wgsl') {
    const assembled = new WGSLShaderAssembler().assembleWGSLShader({ platformInfo, source: sources.wgsl, modules });
    const shader = device.createShader({ id, source: assembled.source, language: 'wgsl' });
    return({
      vertex: shader,
      fragment: shader
    });
  }

  const assembled = new GLSLShaderAssembler().assembleGLSLShaderPair({
    platformInfo,
    vs: explicitGlslVersion(sources.glsl.vertex),
    fs: explicitGlslVersion(sources.glsl.fragment),
    modules
  });
  const vertex = device.createShader({ id: `${id}-vertex`, source: assembled.vs, language: 'glsl', stage: 'vertex' });
  try {
    const fragment = device.createShader({ id: `${id}-fragment`, source: assembled.fs, language: 'glsl', stage: 'fragment' });
    return({
      vertex,
      fragment
    });
  } catch(error) {
    vertex.destroy();
    throw error;
  }
}

/**
 * Shadertools requires the GLSL version that raw Device compilation inferred.
 * @param source
 * @returns GLSL version directive prepended to the source if absent.
 */
function explicitGlslVersion(source: string): string {
  return(/^\s*#version\s+/m.test(source) ? source : `#version 300 es\n${source}`);
}
