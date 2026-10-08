// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_ADDRESS_MODE, GPU_FILTER, GPU_TEXTURE_FORMAT } from '../gpu/constants.ts';
import { SCENE_REPROJECTION } from './constants.ts';
import type { Device } from '@luma.gl/core';
import type { ShaderPassPipeline } from '@luma.gl/shadertools';
import {
  depthAwareBlurShaderPassPipeline, createSSAOShaderPassPipeline, createGTAOShaderPassPipeline,
  createSSGIShaderPassPipeline, createOutlineShaderPassPipeline, createTAAShaderPassPipeline,
  createCameraReprojectionTAAShaderPassPipeline, createMotionBlurShaderPassPipeline,
  createSSRShaderPassPipeline, createVolumetricFogShaderPassPipeline,
  createClusteredVolumetricLightingShaderPassPipeline, createHDRAutoExposureShaderPassPipeline
} from '@luma.gl/effects';
import type { NormalizedSceneEffectsOptions } from './scene-effect-config.ts';

const LINEAR_COLOR_SAMPLER = {
  minFilter: GPU_FILTER.LINEAR, magFilter: GPU_FILTER.LINEAR, addressModeU: GPU_ADDRESS_MODE.CLAMP_TO_EDGE, addressModeV: GPU_ADDRESS_MODE.CLAMP_TO_EDGE
} as const;
const NEAREST_DEPTH_SAMPLER = { ...LINEAR_COLOR_SAMPLER, minFilter: GPU_FILTER.NEAREST, magFilter: GPU_FILTER.NEAREST } as const;
const RAW_DEPTH_HISTORY = new Set([
  'gtaoHistoryDepth', 'ssgiHistoryDepth', 'ssrHistoryDepth', 'taaHistoryDepth', 'cameraReprojectionTaaHistoryDepth'
]);

/** Build upstream pipelines in scene-linear order; no GPU resources are allocated here. */
export function createSceneEffectPipelines(configuration: NormalizedSceneEffectsOptions) {
  const lighting: ShaderPassPipeline[] = [];
  const temporal: ShaderPassPipeline[] = [];
  const finishing: ShaderPassPipeline[] = [];
  if(configuration.ssao) {
    lighting.push(createSSAOShaderPassPipeline(configuration.ssao));
  }
  if(configuration.gtao) {
    lighting.push(createGTAOShaderPassPipeline(configuration.gtao));
  }
  if(configuration.ssgi) {
    lighting.push(createSSGIShaderPassPipeline(configuration.ssgi));
  }
  if(configuration.ssr) {
    lighting.push(createSSRShaderPassPipeline(configuration.ssr));
  }
  if(configuration.clusteredLighting) {
    lighting.push(createClusteredVolumetricLightingShaderPassPipeline(configuration.clusteredLighting));
  }
  if(configuration.heightFog) {
    lighting.push(createVolumetricFogShaderPassPipeline());
  }
  if(configuration.depthAwareBlur) {
    // Per-step tuning avoids overriding the same blur module used by AO and volume denoisers.
    lighting.push({
      ...depthAwareBlurShaderPassPipeline,
      renderTargets: { depthAwareBlurScratch: {} },
      steps: depthAwareBlurShaderPassPipeline.steps.map(step => ({
        ...step, uniforms: { ...configuration.depthAwareBlur, ...step.uniforms }
      }))
    });
  }
  if(configuration.outlines) {
    lighting.push(createOutlineShaderPassPipeline(configuration.outlines));
  }
  if(configuration.taa) {
    temporal.push(configuration.taa.reprojection === SCENE_REPROJECTION.CAMERA
      ? createCameraReprojectionTAAShaderPassPipeline() : createTAAShaderPassPipeline());
  }
  if(configuration.motionBlur) {
    finishing.push(createMotionBlurShaderPassPipeline());
  }
  if(configuration.adaptiveExposure) {
    finishing.push(createHDRAutoExposureShaderPassPipeline(configuration.adaptiveExposure));
  }
  for(const pipeline of [...lighting, ...temporal, ...finishing]) {
    for(const [name, target] of Object.entries(pipeline.renderTargets ?? {})) {
      // Upstream unspecified formats otherwise inherit the canvas and clamp HDR history.
      target.format ??= configuration.colorFormat ?? GPU_TEXTURE_FORMAT.RGBA16FLOAT;
      target.sampler = LINEAR_COLOR_SAMPLER;
      if(RAW_DEPTH_HISTORY.has(name)) {
        // Half-float hardware depth loses far more precision than temporal rejection permits.
        target.format = 'rgba32float';
        target.sampler = NEAREST_DEPTH_SAMPLER;
      }
    }
  }
  return { lighting, temporal, finishing };
}

/** Validate every explicit intermediate format before constructing a renderer. */
export function validateSceneEffectFormats(device: Device, pipelines: ReturnType<typeof createSceneEffectPipelines>) {
  const formats = new Set(Object.values(pipelines).flatMap(group => group.flatMap(pipeline =>
    Object.values(pipeline.renderTargets ?? {}).map(target => target.format!))));
  for(const format of formats) {
    const capabilities = device.getTextureFormatCapabilities(format);
    if(!capabilities.create || !capabilities.render || !capabilities.filter) {
      const hint = format === 'rgba32float' ? ' Temporal depth history requires the float32-filterable WebGPU feature.' : '';
      throw new Error(`Scene effects intermediate ${format} is not supported.${hint}`);
    }
  }
}
