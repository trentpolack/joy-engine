// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Serialized particle renderer families.
 */
export const PARTICLE_RENDERER = Object.freeze({
  SOFT: 'soft',
  STREAK: 'streak',
  RING: 'ring',
  BILLOW: 'billow',
  CIRCLE: 'circle',
  SQUARE: 'square',
  TEXTURED: 'textured',
  FLIPBOOK: 'flipbook'
} as const);

/**
 * Serialized particle parameter types.
 */
export const PARTICLE_PARAMETER_TYPE = Object.freeze({
  SCALAR: 'scalar',
  VECTOR3: 'vector3',
  COLOR: 'color'
} as const);

/**
 * Particle script value categories.
 */
export const PARTICLE_VALUE_KIND = Object.freeze({
  SCALAR: 'scalar',
  VECTOR: 'vector',
  COLOR: 'color'
} as const);
