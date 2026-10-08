// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FILM_GRAIN_NOISE } from './constants.ts';
import type { PostProcessingConfig } from './config.ts';
import { normalizePostProcessingConfig } from './config.ts';

export type { PostProcessingConfig };
export interface PostProcessingProfile {
  version: 1;
  preset: string;
  overrides: Partial<PostProcessingConfig>;
}

/** Original starting points shared by games; a profile stores only its own overrides.
 */
export const POST_PROCESSING_PRESETS: Readonly<Record<string, Readonly<Partial<PostProcessingConfig>>>> = Object.freeze({
  default: Object.freeze({}),
  soft: Object.freeze({ bloomStrength: 0.25, contrast: 0.95, saturation: 0.95 }),
  vivid: Object.freeze({ bloomStrength: 0.6, saturation: 1.2, contrast: 1.05 }),
});

/** Validate serialized data and return a detached, canonically ordered profile.
 * Unknown versions are rejected until a deliberate migration exists.
 * @param value
 */
export function validatePostProcessingProfile(value: unknown): PostProcessingProfile {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError('Post-processing profile must be an object.');
  }
  const record = ((value) as Record<string, unknown>);
  if(Object.keys(record).some(key => !['version', 'preset', 'overrides'].includes(key))) {
    throw new TypeError('Unknown post-processing profile field.');
  }
  if(record.version !== 1) {
    throw new RangeError('Unsupported post-processing profile version; expected 1.');
  }
  if(typeof record.preset !== 'string' || !Object.hasOwn(POST_PROCESSING_PRESETS, record.preset)) {
    throw new RangeError('Unknown post-processing preset.');
  }
  if(!record.overrides || typeof record.overrides !== 'object' || Array.isArray(record.overrides)) {
    throw new TypeError('Post-processing overrides must be an object.');
  }
  const authored = { ... ((record.overrides) as Record<string, unknown>) };
  // Version-one profiles used this label for hash grain without a blue-noise
  // distribution. Migrate the serialized spelling without mutating the input.
  if(authored.filmGrainNoise === 'blue-noise') {
    authored.filmGrainNoise = FILM_GRAIN_NOISE.DECORRELATED;
  }
  const overrides = ((authored) as Partial<PostProcessingConfig>);
  normalizePostProcessingConfig(overrides);
  return {
    version: 1,
    preset: record.preset,
    overrides: Object.fromEntries(Object.entries(overrides).sort(([left], [right]) => left.localeCompare(right))),
  };
}

/** Resolve engine defaults, preset, then game overrides into a new settings record.
 * @param value Serialized authored profile; never retained or modified.
 */
export function resolvePostProcessingProfile(value: unknown): PostProcessingConfig {
  const profile = validatePostProcessingProfile(value);
  return normalizePostProcessingConfig({ ...POST_PROCESSING_PRESETS[profile.preset], ...profile.overrides });
}
