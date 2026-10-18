// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Instance-owned cumulative CPU metrics for particle simulation. Metrics are never simulation input: enabling them
//! cannot change trajectories, and clocks are read only when profiling is enabled.

use crate::asset::CompiledParticleEffect;
use crate::particle_effect::EffectParticle;

/// Per-emitter counters. Times are milliseconds of CPU wall time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleProfileRow {
    /// Emitter id.
    pub id: String,
    /// Time spent spawning (including spawn scripts).
    pub spawn_ms: f64,
    /// Time spent updating, integrating, and retiring particles.
    pub update_ms: f64,
    /// Particles spawned since the last reset.
    pub spawned: u64,
    /// Spawns rejected by capacity limits since the last reset.
    pub dropped: u64,
    /// Live particles after the latest step.
    pub live: u64,
    /// Script instruction units consumed since the last reset.
    pub operations: i64,
}

/// Cumulative metrics for one effect instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleProfile {
    /// Whether metrics are collected.
    pub enabled: bool,
    /// One row per emitter, in definition order.
    pub emitters: Vec<ParticleProfileRow>,
    /// Fixed steps since the last reset.
    pub steps: u64,
    /// Total simulation time in milliseconds, including resets.
    pub simulation_ms: f64,
}

/// Create zeroed metrics for `definition`; the TypeScript `createParticleProfile`.
pub fn create_particle_profile(definition: &CompiledParticleEffect, enabled: bool) -> ParticleProfile {
    ParticleProfile {
        enabled,
        emitters: definition
            .emitters()
            .iter()
            .map(|emitter| ParticleProfileRow { id: emitter.asset.id.clone(), ..Default::default() })
            .collect(),
        steps: 0,
        simulation_ms: 0.0,
    }
}

/// Clear counters in place so inspector references stay valid.
pub fn reset_particle_profile(profile: &mut ParticleProfile) {
    profile.steps = 0;
    profile.simulation_ms = 0.0;
    for row in &mut profile.emitters {
        let id = std::mem::take(&mut row.id);
        *row = ParticleProfileRow { id, ..Default::default() };
    }
}

/// Recount live particles per emitter; retired particles still present after a script failure are excluded.
pub fn refresh_particle_profile_live(profile: &mut ParticleProfile, particles: &[EffectParticle]) {
    if !profile.enabled {
        return;
    }
    for row in &mut profile.emitters {
        row.live = 0;
    }
    for particle in particles {
        if particle.age + 1e-10 < particle.lifetime
            && let Some(row) = profile.emitters.get_mut(particle.emitter_index)
        {
            row.live += 1;
        }
    }
}
