// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { CompiledParticleEffect } from './asset.ts';
import type { EffectParticle } from './particle-effect.ts';

/** Create instance-owned cumulative CPU metrics; never simulation input.
 * @param definition @param enabled
 */
export function createParticleProfile(definition: CompiledParticleEffect, enabled: boolean) {
  return {
    enabled,
    emitters: definition.emitters.map(emitter => ({
      id: emitter.id, spawnMs: 0, updateMs: 0, spawned: 0, dropped: 0, live: 0, operations: 0,
    })),
    steps: 0,
    simulationMs: 0,
  };
}
export type ParticleProfile = ReturnType<typeof createParticleProfile>;

/** Clear owned records in place so inspector references remain valid.
 * @param profile
 */
export function resetParticleProfile(profile: ParticleProfile) {
  profile.steps = 0;
  profile.simulationMs = 0;
  for(const row of profile.emitters) {
    row.spawnMs = 0;
    row.updateMs = 0;
    row.spawned = 0;
    row.dropped = 0;
    row.live = 0;
    row.operations = 0;
  }
}
/** Refresh even after script failure, where the particle array may contain retirees.
 * @param profile
 * @param particles
 */
export function refreshParticleProfileLive(profile: ParticleProfile, particles: readonly EffectParticle[]) {
  if(!profile.enabled) {
    return;
  }
  for(const row of profile.emitters) {
    row.live = 0;
  }
  for(const particle of particles) {
    if(particle.age + 1e-10 < particle.lifetime) {
      profile.emitters[particle.emitterIndex].live++;
    }
  }
}
