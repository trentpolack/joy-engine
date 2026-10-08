// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { POST_PROCESSING_KEY } from './constants.ts';
import type { ShaderPass } from '@luma.gl/shadertools';
import type { Texture } from '@luma.gl/core';
import { fxaa, noise } from '@luma.gl/effects';
import antialiasFragmentSource from '../../../shaders/rendering/postprocessor/modules/antialias.frag.glsl?raw';
import antialiasWebGpuSource from '../../../shaders/rendering/postprocessor/modules/antialias.wgsl?raw';
import agxToneMappingFragmentSource from '../../../shaders/rendering/postprocessor/modules/agx-tone-mapping.frag.glsl?raw';
import agxToneMappingWebGpuSource from '../../../shaders/rendering/postprocessor/modules/agx-tone-mapping.wgsl?raw';
import animatedGrainFragmentSource from '../../../shaders/rendering/postprocessor/modules/animated-grain.frag.glsl?raw';
import animatedGrainWebGpuSource from '../../../shaders/rendering/postprocessor/modules/animated-grain.wgsl?raw';
import linearExposureFragmentSource from '../../../shaders/rendering/postprocessor/modules/linear-exposure.frag.glsl?raw';
import linearExposureWebGpuSource from '../../../shaders/rendering/postprocessor/modules/linear-exposure.wgsl?raw';
import vignetteBlurFragmentSource from '../../../shaders/rendering/postprocessor/modules/vignette-blur.frag.glsl?raw';
import vignetteBlurWebGpuSource from '../../../shaders/rendering/postprocessor/modules/vignette-blur.wgsl?raw';

export type AntialiasProps = {
  amount?: number;
};
export type AntialiasUniforms = {
  amount: number;
};

/**
 * Blends the original scene with luma.gl's portable FXAA result.
 */
export const antialias = {
  name: 'antialias',
  fs: antialiasFragmentSource,
  source: antialiasWebGpuSource,
  dependencies: [fxaa],
  uniforms:  (({}) as AntialiasUniforms),
  uniformTypes: {
    amount: 'f32'
  },
  defaultUniforms: {
    amount: 1
  },
  propTypes: {
    amount: { value: 1, min: 0, max: 1 }
  },
  passes: [{ sampler: true }]
} satisfies ShaderPass<AntialiasProps, AntialiasUniforms>;

export type LinearExposureProps = {
  exposure?: number;
};
export type LinearExposureUniforms = {
  exposure: number;
};

/**
 * Applies exposure without a tone-mapping curve.
 */
export const linearExposure = {
  name: 'linearExposure',
  fs: linearExposureFragmentSource,
  source: linearExposureWebGpuSource,
  uniforms:  (({}) as LinearExposureUniforms),
  uniformTypes: {
    exposure: 'f32'
  },
  defaultUniforms: {
    exposure: 1
  },
  propTypes: {
    exposure: { value: 1, min: 0 }
  },
  passes: [{ filter: true }]
} satisfies ShaderPass<LinearExposureProps, LinearExposureUniforms>;

export type AgxToneMappingProps = {
  exposure?: number;
  look?: number;
};

export type AgxToneMappingUniforms = {
  exposure: number;
  look: number;
};

/**
 * Portable AgX display transform with neutral and punchy looks.
 */
export const agxToneMapping = {
  name: 'agxToneMapping',
  fs: agxToneMappingFragmentSource,
  source: agxToneMappingWebGpuSource,
  uniforms:  (({}) as AgxToneMappingUniforms),
  uniformTypes: {
    exposure: 'f32',
    look: 'f32'
  },
  defaultUniforms: {
    exposure: 1,
    look: 0
  },
  propTypes: {
    exposure: { value: 1, min: 0 },
    look: { value: 0, min: 0, max: 1 }
  },
  passes: [{ filter: true }]
} satisfies ShaderPass<AgxToneMappingProps, AgxToneMappingUniforms>;

export type AnimatedGrainProps = {
  intensity?: number;
  size?: number;
  speed?: number;
  noiseVariant?: number;
  timestamp?: number;
};

export type AnimatedGrainUniforms = {
  intensity: number;
  size: number;
  speed: number;
  noiseVariant: number;
  timestamp: number;
};

/**
 * Animated monochrome film grain built on luma.gl's portable noise hash.
 */
export const animatedGrain = {
  name: 'animatedGrain',
  fs: animatedGrainFragmentSource,
  source: animatedGrainWebGpuSource,
  dependencies: [noise],
  uniforms:  (({}) as AnimatedGrainUniforms),
  uniformTypes: {
    intensity: 'f32',
    size: 'f32',
    speed: 'f32',
    noiseVariant: 'f32',
    timestamp: 'f32'
  },
  defaultUniforms: {
    intensity: 0,
    size: 1.5,
    speed: 18,
    noiseVariant: 0,
    timestamp: 0
  },
  propTypes: {
    intensity: { value: 0, min: 0, max: 1 },
    size: { value: 1.5, min: 0.5, max: 8 },
    speed: { value: 18, min: 0 },
    noiseVariant: { value: 0, min: 0, max: 1 },
    timestamp: { value: 0, min: 0 }
  },
  passes: [{ filter: true }]
} satisfies ShaderPass<AnimatedGrainProps, AnimatedGrainUniforms>;

export type VignetteBlurProps = {
  amount?: number;
  radius?: number;
  blurredTexture?: Texture;
};

export type VignetteBlurUniforms = {
  amount: number;
  radius: number;
};
export type VignetteBlurBindings = {
  blurredTexture: Texture;
};

/**
 * Blends an independently blurred scene into the frame near the vignette edge.
 * The renderer borrows `blurredTexture`; its creator retains ownership.
 */
export const vignetteBlur = {
  name: POST_PROCESSING_KEY.VIGNETTE_BLUR,
  fs: vignetteBlurFragmentSource,
  source: vignetteBlurWebGpuSource,
  bindingLayout: [{ name: 'blurredTexture', group: 0 }],
  uniforms:  (({}) as VignetteBlurUniforms),
  bindings:  (({}) as VignetteBlurBindings),
  uniformTypes: {
    amount: 'f32',
    radius: 'f32'
  },
  defaultUniforms: {
    amount: 0,
    radius: 0.72
  },
  propTypes: {
    amount: { value: 0, min: 0, max: 1 },
    radius: { value: 0.72, min: 0, max: 1 }
  },
  passes: [{ sampler: true }]
} satisfies ShaderPass<VignetteBlurProps, VignetteBlurUniforms, VignetteBlurBindings>;
