// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ParticleEffect } from './particle-effect.ts';

export interface ParticlePointLight {
  x: number;
  y: number;
  z: number;
  r: number;
  g: number;
  b: number;
  range: number;
  emitterIndex: number;
  particleIndex: number;
}

/** Collect an owned bounded list without mutating simulation or consuming randomness.
 * Lights follow XYZ positions; range is world units, RGB is linear HDR multiplied
 * by intensity and clamped alpha. Rank by emitted luminance, then stable birth index.
 * `omitted` counts valid emitting lights excluded by the requested cap (at most 32).
 * @param effect
 * @param [maxLights] Integer from 0 to 32; larger integers clamp to 32.
 */
export function collectParticleLights(effect: ParticleEffect, maxLights: number = 32): {
    lights: ParticlePointLight[];
    omitted: number;
} {
  if(!Number.isInteger(maxLights) || maxLights < 0) {
    throw new RangeError('Particle light limit must be a nonnegative integer.');
  }
  const limit = Math.min(32, maxLights);

  const candidates: ParticlePointLight[] = [];
  for(const particle of effect.particles) {
    if(!effect.definition.emitters[particle.emitterIndex].light ||
      particle.age + 1e-10 >= particle.lifetime || particle.alpha <= 0 ||
      particle.lightIntensity <= 0 || particle.lightIntensity > 1000 ||
      particle.lightRange < 0.01 || particle.lightRange > 10000) {
      continue;
    }
    const values = [particle.x, particle.y, particle.z, particle.r, particle.g,
      particle.b, particle.alpha, particle.lightIntensity, particle.lightRange];
    if(!values.every(Number.isFinite)) {
      continue;
    }
    const strength = particle.lightIntensity * Math.min(1, particle.alpha);
    const r = Math.max(0, particle.r) * strength;
    const g = Math.max(0, particle.g) * strength;
    const b = Math.max(0, particle.b) * strength;
    if(r + g + b <= 0) {
      continue;
    }
    candidates.push({ x: particle.x, y: particle.y, z: particle.z, r, g, b,
      range: particle.lightRange, emitterIndex: particle.emitterIndex, particleIndex: particle.index });
  }
  candidates.sort((a, b) => luminance(b) - luminance(a) || a.particleIndex - b.particleIndex);
  return { lights: candidates.slice(0, limit), omitted: Math.max(0, candidates.length - limit) };
}

/** @param light */
function luminance(light: ParticlePointLight) {
  return light.r * 0.2126 + light.g * 0.7152 + light.b * 0.0722;
}
