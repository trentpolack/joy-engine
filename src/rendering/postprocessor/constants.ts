// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Supported tone-mapping operators.
 */
export const TONE_MAPPER = Object.freeze({
  NONE: 'none',
  AGX: 'agx',
  ACES: 'aces'
} as const);

/**
 * Serialized post-processing configuration property names.
 */
export const POST_PROCESSING_KEY = Object.freeze({
  TONE_MAPPER: 'toneMapper',
  ANTIALIAS_STRENGTH: 'antialiasStrength',
  EXPOSURE: 'exposure',
  BLOOM_STRENGTH: 'bloomStrength',
  BLOOM_RADIUS: 'bloomRadius',
  BLOOM_THRESHOLD: 'bloomThreshold',
  BLOOM_KNEE: 'bloomKnee',
  BLOOM_ANAMORPHIC: 'bloomAnamorphic',
  BLOOM_QUALITY: 'bloomQuality',
  BLOOM_SCATTER: 'bloomScatter',
  BLOOM_LENS_FLARE: 'bloomLensFlare',
  BRIGHTNESS: 'brightness',
  VIBRANCE: 'vibrance',
  BLUR_RADIUS: 'blurRadius',
  SATURATION: 'saturation',
  CONTRAST: 'contrast',
  FILM_GRAIN: 'filmGrain',
  FILM_GRAIN_SIZE: 'filmGrainSize',
  FILM_GRAIN_SPEED: 'filmGrainSpeed',
  FILM_GRAIN_NOISE: 'filmGrainNoise',
  VIGNETTE_STRENGTH: 'vignetteStrength',
  VIGNETTE_RADIUS: 'vignetteRadius',
  VIGNETTE_BLUR: 'vignetteBlur',
  AGX_LOOK: 'agxLook'
} as const);

/**
 * Bloom pyramid quality presets.
 */
export const BLOOM_QUALITY = Object.freeze({
  LOW: 'low',
  MEDIUM: 'medium',
  HIGH: 'high',
  ULTRA: 'ultra'
} as const);

/**
 * AgX grading looks.
 */
export const AGX_LOOK = Object.freeze({
  NEUTRAL: 'neutral',
  PUNCHY: 'punchy'
} as const);

/**
 * Film-grain noise algorithms.
 */
export const FILM_GRAIN_NOISE = Object.freeze({
  ANALOG: 'analog',
  DECORRELATED: 'decorrelated'
} as const);
