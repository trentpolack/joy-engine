// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { EnvironmentFrame } from '../../environment/environment-system.ts';

export const ENVIRONMENT_UNIFORM_BYTES = 256;

/**
 * Pack an environment snapshot into a portable 16-byte-aligned GPU uniform block.
 * Directions and wind are Y-up world-space values; distance fields use world units;
 * RGB values are linear. The returned array is caller-owned and safe to upload.
 * @param frame Borrowed immutable snapshot.
 */
export function createEnvironmentUniforms(frame: EnvironmentFrame): Float32Array {
  const data = new Float32Array(ENVIRONMENT_UNIFORM_BYTES/Float32Array.BYTES_PER_ELEMENT);
  data.set([...frame.sun.direction, frame.sun.intensity], 0);
  data.set([...frame.sun.color, frame.sun.angularRadius], 4);
  data.set([...frame.moon.direction, frame.moon.intensity], 8);
  data.set([...frame.moon.color, frame.moon.angularRadius], 12);
  data.set([...frame.skylight.color, frame.skylight.intensity], 16);
  data.set([...frame.wind.velocity, frame.elapsedSeconds], 20);
  data.set([frame.atmosphere.rayleigh, frame.atmosphere.mie, frame.atmosphere.ozone, frame.atmosphere.density], 24);
  data.set([frame.atmosphere.fogDensity, frame.clouds.coverage, frame.clouds.density, frame.clouds.shadowStrength], 28);
  data.set([frame.clouds.altitude, frame.clouds.thickness, frame.lightShafts.intensity, frame.lightShafts.decay], 32);
  data.set([frame.lightShafts.exposure, frame.ambientOcclusion.intensity, frame.ambientOcclusion.radius, frame.timeOfDay], 36);
  if(!data.every(Number.isFinite)) {
    throw new RangeError('Environment uniforms must fit finite float32 values.');
  }
  return data;
}

/**
 * Adapt the environment's primary atmosphere light and skylight to a PBR frame.
 * The returned record borrows the frame's immutable arrays and can be spread into
 * a PbrFrame without allocating GPU resources.
 * @param frame
 */
export function createEnvironmentLighting(frame: EnvironmentFrame) {
  return Object.freeze({
    lightDirection: frame.primaryLight.direction,
    lightColor: Object.freeze(frame.primaryLight.color.map(value => value*frame.primaryLight.intensity)),
    ambientColor: Object.freeze(frame.skylight.color.map(value => value*frame.skylight.intensity))
  });
}
