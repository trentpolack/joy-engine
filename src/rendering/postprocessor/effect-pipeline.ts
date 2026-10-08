// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { GPU_ADDRESS_MODE, GPU_FILTER, GPU_TEXTURE_FORMAT } from '../gpu/constants.ts';
import { AGX_LOOK, FILM_GRAIN_NOISE, TONE_MAPPER } from './constants.ts';
import type { PostProcessingConfig } from './config.ts';
import type { ShaderPassPipeline, ShaderPass } from '@luma.gl/shadertools';
import {
  brightnessContrast,
  createBloomShaderPassPipeline,
  gaussianBlur,
  hueSaturation,
  toneMapping,
  vibrance,
  vignette
} from '@luma.gl/effects';
import { agxToneMapping, animatedGrain, antialias, linearExposure, vignetteBlur } from './supplemental-passes.ts';

export type { PostProcessingConfig };
export type { ShaderPassPipeline };
export type { ShaderPass };

// A radius of 0.025 at 720 pixels maps to the upstream default eight-texel
// kernel. The pyramid supplies the broad falloff; kernel taps stay contiguous.
const BLOOM_RADIUS_SCALE = 8/(720*0.025);
const MAX_BLOOM_RADIUS = 24;
const MAX_GAUSSIAN_RADIUS = 32;
const LINEAR_COLOR_SAMPLER = Object.freeze({
  minFilter: GPU_FILTER.LINEAR,
  magFilter: GPU_FILTER.LINEAR,
  addressModeU: GPU_ADDRESS_MODE.CLAMP_TO_EDGE,
  addressModeV: GPU_ADDRESS_MODE.CLAMP_TO_EDGE
});

/**
 * Build upstream effect descriptions. Changing graph shape creates a new owner;
 * ordinary color/intensity controls are supplied as frame uniforms instead.
 * @param config
 * @param shortestAxis Drawing-buffer pixels, including display density.
 * @param format
 */
export function createEffectPipeline(config: PostProcessingConfig, shortestAxis: number, format: typeof GPU_TEXTURE_FORMAT.RGBA16FLOAT | typeof GPU_TEXTURE_FORMAT.RGBA8UNORM): (ShaderPass | ShaderPassPipeline)[] {

  const passes: (ShaderPass | ShaderPassPipeline)[] = [];
  if(config.bloomStrength > 0) {
    passes.push(createBloomShaderPassPipeline({
      quality: config.bloomQuality,
      colorFormat: format,
      radius: Math.min(MAX_BLOOM_RADIUS, config.bloomRadius*shortestAxis*BLOOM_RADIUS_SCALE),
      anamorphicRatio: config.bloomAnamorphic,
      reconstruction: 'bicubic',
      // Retain the upstream fragment fallback when compute/storage is unavailable.
      downsample: 'auto',
      lens: config.bloomLensFlare > 0 ? {
        starburstIntensity: config.bloomLensFlare,
        starburstSpikes: 2,
        starburstLength: config.bloomRadius*shortestAxis*4
      } : undefined
    }));
  }
  if(config.blurRadius > 0) {
    passes.push({ ...gaussianBlur, passes: [
      { sampler: true, uniforms: { delta: [1, 0], radius: Math.min(MAX_GAUSSIAN_RADIUS, config.blurRadius*shortestAxis) } },
      { sampler: true, uniforms: { delta: [0, 1], radius: Math.min(MAX_GAUSSIAN_RADIUS, config.blurRadius*shortestAxis) } }
    ] });
  }
  if(config.vignetteBlur > 0) {
    passes.push(createVignetteBlurPipeline(format));
  }
  passes.push(config.toneMapper === TONE_MAPPER.AGX ? agxToneMapping : config.toneMapper === TONE_MAPPER.ACES ? toneMapping : linearExposure);
  passes.push(hueSaturation, brightnessContrast, vibrance);
  if(config.vignetteStrength > 0) {
    passes.push(vignette);
  }
  if(config.antialiasStrength > 0) {
    passes.push(antialias);
  }
  if(config.filmGrain > 0) {
    passes.push(animatedGrain);
  }
  return passes;
}

/**
 * Only values baked into graph topology or per-axis upstream subpasses belong
 * here. Exposure, grade, grain animation and bloom intensity never rebuild it.
 * @param config
 * @param shortestAxis
 */
export function effectPipelineKey(config: PostProcessingConfig, shortestAxis: number) {
  return JSON.stringify([
    shortestAxis, config.toneMapper, config.bloomStrength > 0,
    config.bloomRadius, config.bloomAnamorphic, config.bloomQuality,
    config.bloomLensFlare > 0, config.blurRadius, config.vignetteBlur > 0,
    config.vignetteStrength > 0, config.antialiasStrength > 0, config.filmGrain > 0
  ]);
}

/**
 * Translate saved Joy Engine controls into upstream module units.
 * @param config
 * @param shortestAxis Drawing-buffer pixels.
 * @param timeSeconds
 */
export function createEffectUniforms(config: PostProcessingConfig, shortestAxis: number, timeSeconds: number): Record<string, Record<string, unknown>> {
  const contrast = config.contrast > 1 ? 1 - 1/config.contrast : config.contrast - 1;
  const saturation = config.saturation > 1 ? 1.001 - 1/config.saturation : config.saturation - 1;
  return {
    bloomExtract: { threshold: config.bloomThreshold, softKnee: config.bloomKnee/Math.max(config.bloomThreshold, 0.00001), exposure: config.exposure },
    bloomUpsample: { scatter: config.bloomScatter },
    bloomComposite: { intensity: config.bloomStrength },
    bloomLens: { starburstIntensity: config.bloomLensFlare },
    toneMapping: { exposure: config.exposure },
    agxToneMapping: { exposure: config.exposure, look: config.agxLook === AGX_LOOK.PUNCHY ? 1 : 0 },
    linearExposure: { exposure: config.exposure },
    hueSaturation: { hue: 0, saturation },
    brightnessContrast: { brightness: config.brightness - 1, contrast },
    vibrance: { amount: config.vibrance },
    vignette: { radius: config.vignetteRadius, amount: config.vignetteStrength },
    vignetteBlur: { amount: config.vignetteBlur, radius: config.vignetteRadius },
    antialias: { amount: config.antialiasStrength },
    animatedGrain: { intensity: config.filmGrain, size: config.filmGrainSize, speed: config.filmGrainSpeed, timestamp: timeSeconds, noiseVariant: config.filmGrainNoise === FILM_GRAIN_NOISE.DECORRELATED ? 1 : 0 }
  };
}

/** @param format */
function createVignetteBlurPipeline(format: typeof GPU_TEXTURE_FORMAT.RGBA16FLOAT | typeof GPU_TEXTURE_FORMAT.RGBA8UNORM): ShaderPassPipeline {
  // Named outputs leave `previous` pointing to the unblurred scene, allowing
  // the mask to blend the Gaussian result only near the vignette edge.
  const horizontal = { ...gaussianBlur, passes: [{ sampler: true, uniforms: { delta: [1, 0], radius: 6 } }] };
  const vertical = { ...gaussianBlur, passes: [{ sampler: true, uniforms: { delta: [0, 1], radius: 6 } }] };
  return {
    name: 'vignetteBlurPipeline',
    renderTargets: {
      vignetteHorizontal: { format, scale: [0.5, 0.5], sampler: LINEAR_COLOR_SAMPLER },
      vignetteSoft: { format, scale: [0.5, 0.5], sampler: LINEAR_COLOR_SAMPLER }
    },
    steps: [
      { shaderPass: horizontal, inputs: { sourceTexture: 'previous' }, output: 'vignetteHorizontal' },
      { shaderPass: vertical, inputs: { sourceTexture: 'vignetteHorizontal' }, output: 'vignetteSoft' },
      { shaderPass: vignetteBlur, inputs: { sourceTexture: 'previous', blurredTexture: 'vignetteSoft' }, output: 'previous' }
    ]
  };
}
