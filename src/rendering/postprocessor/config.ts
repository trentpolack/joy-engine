// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { AGX_LOOK, BLOOM_QUALITY, FILM_GRAIN_NOISE, POST_PROCESSING_KEY, TONE_MAPPER } from './constants.ts';

export type PostProcessingConfig = {
  toneMapper: typeof TONE_MAPPER.AGX | typeof TONE_MAPPER.ACES | typeof TONE_MAPPER.NONE;
  antialiasStrength: number;
  exposure: number;
  bloomStrength: number;
  bloomRadius: number;
  bloomThreshold: number;
  bloomKnee: number;
  bloomAnamorphic: number;
  bloomQuality: typeof BLOOM_QUALITY.LOW | typeof BLOOM_QUALITY.MEDIUM | typeof BLOOM_QUALITY.HIGH | typeof BLOOM_QUALITY.ULTRA;
  bloomScatter: number;
  bloomLensFlare: number;
  brightness: number;
  vibrance: number;
  blurRadius: number;
  saturation: number;
  contrast: number;
  filmGrain: number;
  filmGrainSize: number;
  filmGrainSpeed: number;
  filmGrainNoise: typeof FILM_GRAIN_NOISE.ANALOG | typeof FILM_GRAIN_NOISE.DECORRELATED;
  vignetteStrength: number;
  vignetteRadius: number;
  vignetteBlur: number;
  agxLook: typeof AGX_LOOK.NEUTRAL | typeof AGX_LOOK.PUNCHY;
};

export type PostProcessingKey = keyof PostProcessingConfig;
export type PostProcessingControl = {
  key: PostProcessingKey;
  label: string;
  group: string;
  description: string;
  defaultValue: number | string;
  minimum?: number;
  exclusiveMinimum?: boolean;
  maximum?: number;
  sliderMaximum?: number;
  step?: number;
  choices?: readonly string[];
};

/** One source for validation, engine defaults, and live controls. Uniform-only controls retain resources; graph controls rebuild the effect chain.
 */
export const POST_PROCESSING_SCHEMA: readonly Readonly<PostProcessingControl>[] = Object.freeze(((([
  { key: POST_PROCESSING_KEY.TONE_MAPPER, label: 'Tone Mapper', group: 'Display', defaultValue: TONE_MAPPER.AGX, choices: Object.freeze([TONE_MAPPER.AGX, TONE_MAPPER.ACES, TONE_MAPPER.NONE]), description: 'Display transform after linear HDR composition.' },
  { key: POST_PROCESSING_KEY.AGX_LOOK, label: 'AgX Look', group: 'Display', defaultValue: AGX_LOOK.PUNCHY, choices: Object.freeze([AGX_LOOK.NEUTRAL, AGX_LOOK.PUNCHY]), description: 'Punchy adds the AgX power and saturation look before the display transform.' },
  { key: POST_PROCESSING_KEY.EXPOSURE, label: 'Exposure', group: 'Display', defaultValue: 1, minimum: 0, sliderMaximum: 4, step: 0.01, description: 'Linear scene exposure before tone mapping.' },
  { key: POST_PROCESSING_KEY.ANTIALIAS_STRENGTH, label: 'Antialias Strength', group: 'Display', defaultValue: 1, minimum: 0, maximum: 1, sliderMaximum: 1, step: 0.01, description: 'Edge-only scene smoothing; zero disables it.' },
  { key: POST_PROCESSING_KEY.BLOOM_STRENGTH, label: 'Strength', group: 'Bloom', defaultValue: 0, minimum: 0, sliderMaximum: 3, step: 0.01, description: 'Bright-pass glow strength; zero disables its visual contribution.' },
  { key: POST_PROCESSING_KEY.BLOOM_RADIUS, label: 'Radius', group: 'Bloom', defaultValue: 0.025, minimum: 0, maximum: 0.25, sliderMaximum: 0.1, step: 0.001, description: 'Blur reach as a fraction of the shorter viewport axis.' },
  { key: POST_PROCESSING_KEY.BLOOM_THRESHOLD, label: 'Threshold', group: 'Bloom', defaultValue: 1, minimum: 0, sliderMaximum: 4, step: 0.01, description: 'Linear luminance at which glow begins.' },
  { key: POST_PROCESSING_KEY.BLOOM_ANAMORPHIC, label: 'Anamorphic', group: 'Bloom', defaultValue: 0, minimum: 0, maximum: 1, sliderMaximum: 1, step: 0.01, description: 'Stretches bloom horizontally into a lens-flare shape.' },
  { key: POST_PROCESSING_KEY.BLOOM_QUALITY, label: 'Quality', group: 'Bloom', defaultValue: BLOOM_QUALITY.HIGH, choices: Object.freeze([BLOOM_QUALITY.LOW, BLOOM_QUALITY.MEDIUM, BLOOM_QUALITY.HIGH, BLOOM_QUALITY.ULTRA]), description: 'Two to five HDR pyramid levels; higher levels retain broader, smoother glow.' },
  { key: POST_PROCESSING_KEY.BLOOM_SCATTER, label: 'Scatter', group: 'Bloom', defaultValue: 0.55, minimum: 0, maximum: 1, sliderMaximum: 1, step: 0.01, description: 'Contribution of the wider bloom levels during reconstruction.' },
  { key: POST_PROCESSING_KEY.BLOOM_LENS_FLARE, label: 'Lens Streaks', group: 'Bloom', defaultValue: 0, minimum: 0, sliderMaximum: 3, step: 0.01, description: 'Independent horizontal diffraction streaks generated from HDR highlights.' },
  { key: POST_PROCESSING_KEY.BLOOM_KNEE, label: 'Soft Knee', group: 'Bloom', defaultValue: 0.35, minimum: 0, exclusiveMinimum: true, sliderMaximum: 2, step: 0.01, description: 'Width of the transition into bloom; must be positive.' },
  { key: POST_PROCESSING_KEY.SATURATION, label: 'Saturation', group: 'Grade', defaultValue: 1, minimum: 0, sliderMaximum: 3, step: 0.01, description: 'Display saturation after tone mapping.' },
  { key: POST_PROCESSING_KEY.CONTRAST, label: 'Contrast', group: 'Grade', defaultValue: 1, minimum: 0, sliderMaximum: 3, step: 0.01, description: 'Display contrast around middle gray.' },
  { key: POST_PROCESSING_KEY.BRIGHTNESS, label: 'Brightness', group: 'Grade', defaultValue: 1, minimum: 0, maximum: 2, sliderMaximum: 2, step: 0.01, description: 'Display brightness multiplier; zero is black and one preserves the tone-mapped image.' },
  { key: POST_PROCESSING_KEY.VIBRANCE, label: 'Vibrance', group: 'Grade', defaultValue: 0, minimum: -1, maximum: 1, step: 0.01, description: 'Adjusts muted colors more strongly than saturated colors.' },
  { key: POST_PROCESSING_KEY.BLUR_RADIUS, label: 'Gaussian Radius', group: 'Blur', defaultValue: 0, minimum: 0, maximum: 0.05, sliderMaximum: 0.05, step: 0.001, description: 'Full-image Gaussian blur as a fraction of the shorter viewport axis.' },
  { key: POST_PROCESSING_KEY.FILM_GRAIN, label: 'Intensity', group: 'Film Grain', defaultValue: 0, minimum: 0, sliderMaximum: 0.2, step: 0.001, description: 'Luminance-aware grain intensity.' },
  { key: POST_PROCESSING_KEY.FILM_GRAIN_SIZE, label: 'Size', group: 'Film Grain', defaultValue: 1.5, minimum: 0.5, maximum: 8, sliderMaximum: 8, step: 0.1, description: 'Noise-cell size in physical pixels.' },
  { key: POST_PROCESSING_KEY.FILM_GRAIN_SPEED, label: 'Speed', group: 'Film Grain', defaultValue: 18, minimum: 0, sliderMaximum: 60, step: 0.5, description: 'Noise animation rate in cycles per second.' },
  { key: POST_PROCESSING_KEY.FILM_GRAIN_NOISE, label: 'Noise', group: 'Film Grain', defaultValue: FILM_GRAIN_NOISE.DECORRELATED, choices: Object.freeze([FILM_GRAIN_NOISE.ANALOG, FILM_GRAIN_NOISE.DECORRELATED]), description: 'Analog hash or averaged decorrelated hash grain; neither claims a blue-noise spectrum.' },
  { key: POST_PROCESSING_KEY.VIGNETTE_STRENGTH, label: 'Strength', group: 'Vignette', defaultValue: 0, minimum: 0, sliderMaximum: 1, step: 0.01, description: 'Edge darkening strength.' },
  { key: POST_PROCESSING_KEY.VIGNETTE_BLUR, label: 'Blur', group: 'Vignette', defaultValue: 0, minimum: 0, maximum: 1, sliderMaximum: 1, step: 0.01, description: 'Softens the image toward the vignette edge.' },
  { key: POST_PROCESSING_KEY.VIGNETTE_RADIUS, label: 'Radius', group: 'Vignette', defaultValue: 0.72, minimum: 0, exclusiveMinimum: true, maximum: 1, sliderMaximum: 1, step: 0.01, description: 'Normalized radius where edge darkening begins.' },
]) as PostProcessingControl[])).map(control => Object.freeze(control)));

export const DEFAULT_POST_PROCESSING: Readonly<PostProcessingConfig> = Object.freeze(((
  Object.fromEntries(POST_PROCESSING_SCHEMA.map(control => [control.key, control.defaultValue]))) as PostProcessingConfig
));

/**
 * Validate a complete replacement or partial authored config without mutating it.
 * Unknown keys and invalid values throw before the caller changes live state.
 * @param config
 * @returns A new, complete record owned by the caller.
 */
export function normalizePostProcessingConfig(config: Partial<PostProcessingConfig>): PostProcessingConfig {
  if(!config || typeof config !== 'object' || Array.isArray(config)) {
    throw new TypeError('Post-processing settings must be an object.');
  }
  for(const key of Object.keys(config)) {
    if(!POST_PROCESSING_SCHEMA.some(control => control.key === key)) {
      throw new TypeError(`Unknown post-processing setting: ${key}.`);
    }
  }
  const normalized = { ...DEFAULT_POST_PROCESSING, ...config };
  for(const control of POST_PROCESSING_SCHEMA) {
    const value = normalized[control.key];
    if(control.choices) {
      if(typeof value !== 'string' || !control.choices.includes(value)) {
        throw new RangeError(`Post-processing ${control.key} must be one of: ${control.choices.join(', ')}.`);
      }
      continue;
    }
    if(typeof value !== 'number' || !Number.isFinite(value)) {
      throw new TypeError(`Post-processing ${control.key} must be finite.`);
    }
    if(control.minimum !== undefined && (control.exclusiveMinimum ? value <= control.minimum : value < control.minimum)) {
      const requirement = control.exclusiveMinimum ? 'must be greater than zero' : 'cannot be negative';
      throw new RangeError(`Post-processing ${control.key} ${requirement}.`);
    }
    if(control.maximum !== undefined && value > control.maximum) {
      throw new RangeError(`Post-processing ${control.key} exceeds ${control.maximum}.`);
    }
  }
  return normalized;
}

/**
 * Build the five vec4 values appended to a game's uniform block.
 * @param config
 * @param width Drawing-buffer width.
 * @param height Drawing-buffer height.
 * @param timeSeconds Animation clock in seconds.
 */
export function createPostProcessingUniforms(config: PostProcessingConfig, width: number, height: number, timeSeconds: number) {
  return new Float32Array([
    width, height, timeSeconds, config.vignetteStrength,
    config.exposure, config.bloomStrength, config.bloomThreshold, config.filmGrain,
    config.bloomKnee, config.saturation, config.contrast, config.vignetteRadius,
    config.bloomRadius, config.bloomAnamorphic, config.vignetteBlur, config.agxLook === AGX_LOOK.PUNCHY ? 1 : 0,
    config.filmGrainSize, config.filmGrainSpeed, config.filmGrainNoise === FILM_GRAIN_NOISE.DECORRELATED ? 1 : 0, 0,
  ]);
}

/**
 * Encode the named public option without exposing shader-specific constants.
 * @param toneMapper
 */
export function toneMapperIndex(toneMapper: PostProcessingConfig[typeof POST_PROCESSING_KEY.TONE_MAPPER]) {
  if(toneMapper === TONE_MAPPER.NONE) {
    return 0;
  } else if(toneMapper === TONE_MAPPER.ACES) {
    return 1;
  }

  return 2;
}

/**
 * Complete the engine-owned uniform tail in a scene shader while preserving
 * linear scene color. Display effects are authored exclusively in the final
 * resolve shaders; geometry shaders must never tone-map individual draws.
 * Without an offscreen pass, omit the uniform tail while still completing the
 * fragment output. The returned sources are owned copies of the input template.
 * @param sources
 * @param [options] Match the renderer's uniform allocation.
 */
export function addPostProcessingShaders(sources: {
    wgsl: string;
    glsl: {
        vertex: string;
        fragment: string;
    };
}, {includeUniforms = true}: {
    includeUniforms?: boolean;
} = {}) {
  for(const marker of ['JOY_POST_UNIFORMS_MACRO', 'JOY_POST_DECLARATIONS_MACRO', 'JOY_POST_APPLY_MACRO']) {
    if(!sources.wgsl.includes(marker) || !sources.glsl.fragment.includes(marker)) {
      throw new Error(`Post-processing shader is missing the ${marker} marker.`);
    }
  }

  for(const shader of [sources.glsl.vertex, sources.glsl.fragment]) {
    if(!shader.includes('JOY_POST_UNIFORMS_MACRO')) {
      throw new Error('Post-processing GLSL shader is missing the JOY_POST_UNIFORMS_MACRO marker.');
    }
  }

  return {
    wgsl: sources.wgsl
      .replace('// JOY_POST_UNIFORMS_MACRO', includeUniforms ? 'postViewport: vec4f,\n  postEffects: vec4f,\n  postGrade: vec4f,\n  postBloom: vec4f,\n  postGrain: vec4f,' : '')
      .replace('// JOY_POST_DECLARATIONS_MACRO', '')
      .replace('// JOY_POST_APPLY_MACRO', 'return sourceColor;'),
    glsl: {
      vertex: sources.glsl.vertex.replace('// JOY_POST_UNIFORMS_MACRO', includeUniforms ? 'vec4 postViewport;\n  vec4 postEffects;\n  vec4 postGrade;\n  vec4 postBloom;\n  vec4 postGrain;' : ''),
      fragment: sources.glsl.fragment
        .replace('// JOY_POST_UNIFORMS_MACRO', includeUniforms ? 'vec4 postViewport;\n  vec4 postEffects;\n  vec4 postGrade;\n  vec4 postBloom;\n  vec4 postGrain;' : '')
        .replace('// JOY_POST_DECLARATIONS_MACRO', '')
        .replace('// JOY_POST_APPLY_MACRO', 'fragmentColor = sourceColor;'),
    },
  };
}
