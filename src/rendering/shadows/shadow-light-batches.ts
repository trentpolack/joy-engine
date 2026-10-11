// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {PointLight} from '../pbr/point-lights.ts';
import {MAX_POINT_LIGHTS} from '../pbr/point-lights.ts';

export const SHADOW_LIGHTS_PER_BATCH = 4;

/** Borrowed lights, original indices, and compact caster slots (-1 means illumination only). */
export interface ShadowLightBatch {
  lights: readonly PointLight[];
  indices: number[];
  shadowIndices: number[];
}

/**
 * Split direct illumination without dropping lights or changing input order.
 * An empty scene still needs a base pass for directional and ambient light.
 */
export function prepareShadowLightBatches(lights: readonly PointLight[]): ShadowLightBatch[] {
  if(lights.length > MAX_POINT_LIGHTS) {
    throw new RangeError('Shadow lighting supports at most 32 point lights.');
  }
  const batches: ShadowLightBatch[] = [];
  for(let first = 0; first < Math.max(1, lights.length); first+= SHADOW_LIGHTS_PER_BATCH) {
    const group = lights.slice(first, first + SHADOW_LIGHTS_PER_BATCH);
    let shadowIndex = 0;
    batches.push({
      lights: group,
      indices: group.map((_, index) => first + index),
      shadowIndices: group.map(light => light.castsShadow === false ? -1 : shadowIndex++)
    });
  }
  return batches;
}
