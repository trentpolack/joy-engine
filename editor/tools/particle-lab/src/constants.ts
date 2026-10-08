// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Particle preview performance inspection modes.
 */
export const PARTICLE_PROFILE_MODE = Object.freeze({
  DISABLED: 'disabled',
  BASIC: 'basic',
  DETAILED: 'detailed'
} as const);

/**
 * Emitter identities referenced by the bundled particle examples.
 */
export const PARTICLE_EXAMPLE_EMITTER = Object.freeze({
  FLASH: 'flash',
  SPARKS: 'sparks',
  BILLOW: 'billow',
  SMOKE: 'smoke',
  ORBIT: 'orbit',
  CORE: 'core',
  DUST: 'dust'
} as const);

/**
 * Insertion preset identifiers owned by Particle Lab.
 */
export const PARTICLE_EMITTER_PRESET = Object.freeze({
  SCRATCH: 'scratch',
  SPRAY: 'spray',
  SPARKS: 'sparks',
  RING: 'ring',
  SMOKE: 'smoke'
} as const);
