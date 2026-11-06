// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/particle_pool.rs`.

use serde_json::json;

use super::*;
use crate::asset::compile_particle_effect;

/// A one-burst effect whose particles live 0.1 seconds, so it finishes after a few updates.
fn burst() -> CompiledParticleEffect {
    compile_particle_effect(&json!({
        "version": 1, "name": "Burst", "maxParticles": 8, "parameters": {},
        "emitters": [{"id": "burst", "enabled": true, "root": true, "duration": 0, "rate": 0,
            "bursts": [{"time": 0, "count": 4}], "lifetime": 0.1, "renderer": "soft",
            "spawn": "velocity = vector(rand(-1, 1), 1, 0); size = 0.2;", "update": ""}]
    }))
    .unwrap()
}

#[test]
fn the_pool_stays_within_budget_seeds_in_sequence_and_drops_finished_effects() {
    let definition = burst();
    let mut pool = ParticleEffectPool::new(EffectPoolOptions {
        capacity: 3,
        first_seed: 10,
    });

    // Five cues into a budget of three keep the newest three; seeds follow the counter in spawn order.
    for cue in 0..5 {
        pool.spawn(&definition, [cue as f64, 0.0, 0.0], cue).unwrap();
    }
    assert_eq!(pool.len(), 3);
    assert_eq!(pool.iter().map(|entry| entry.data).collect::<Vec<_>>(), vec![2, 3, 4]);
    assert_eq!(pool.next_seed(), 15, "every spawn drew one seed");

    // Paused updates hold; a frame longer than the clamp still advances instead of failing every effect.
    pool.update(0.0);
    assert!(pool.iter().all(|entry| entry.age_seconds == 0.0));
    pool.update(10.0);
    assert!(pool.is_empty(), "burst effects finish and leave the pool");

    // Clearing continues the cosmetic sequence; restarting rewinds it, so a restarted level replays the same effects.
    pool.spawn(&definition, [0.0; 3], 0).unwrap();
    pool.clear();
    assert!(pool.is_empty());
    assert_eq!(pool.next_seed(), 17);
    pool.spawn(&definition, [0.0; 3], 0).unwrap();
    pool.restart();
    assert!(pool.is_empty());
    assert_eq!(pool.next_seed(), 10);
}
