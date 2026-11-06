// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A bounded pool of live particle effects for a game's one-shot cues (impacts, bursts, and casts).
//!
//! Every game that played effects in response to its events owned the same loop: evict the oldest effect when the
//! budget is full, hand each new effect the next value of a cosmetic seed counter, advance every effect, and drop the
//! ones that finished. The pool owns that loop, so games keep only what is theirs: which definition an event plays,
//! where it plays, and the per-effect data (a color or an age-driven flash) they draw alongside it.
//!
//! Seeds: [`ParticleEffectPool::spawn`] draws the pool's own counter, which starts at [`EffectPoolOptions::first_seed`]
//! and restarts on [`ParticleEffectPool::restart`], so effects never consume gameplay randomness. A game that seeds
//! effects from a stream of its own passes complete options through [`ParticleEffectPool::spawn_with`].
//!
//! Failures: an effect whose script fails while advancing is logged, destroyed, and dropped, so one broken cue never
//! stops a frame. Construction failures are returned, because they usually mean a bad definition or origin.

use std::collections::VecDeque;

use crate::asset::CompiledParticleEffect;
use crate::particle_effect::{
    ParticleEffect,
    ParticleEffectError,
    ParticleEffectOptions,
};

/// The longest real-time step [`ParticleEffectPool::update`] hands its effects, in seconds; longer frames (a paused
/// tab, a debugger stop) are clamped to it, matching [`ParticleEffect::update`]'s accepted range.
pub const MAXIMUM_POOL_UPDATE_SECONDS: f64 = 0.25;

/// Budget and seeding for a [`ParticleEffectPool`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectPoolOptions {
    /// Most live effects; spawning into a full pool first evicts the oldest. At least one.
    pub capacity: usize,
    /// First value of the pool's cosmetic seed counter, and the value [`ParticleEffectPool::restart`] restores.
    pub first_seed: i64,
}

impl Default for EffectPoolOptions {
    fn default() -> Self {
        Self {
            capacity: 32,
            first_seed: 1,
        }
    }
}

/// One live effect, how long it has been alive, and the owner's data for it.
#[derive(Debug)]
pub struct PooledEffect<T> {
    /// The effect; read it to draw, or stop it early.
    pub effect: ParticleEffect,
    /// Seconds of [`ParticleEffectPool::update`] time, or fixed steps' time, since it was spawned.
    pub age_seconds: f64,
    /// The owner's per-effect data (what played it, its color, or its light).
    pub data: T,
}

/// Owns live effects in spawn order (oldest first), within a fixed budget.
#[derive(Debug)]
pub struct ParticleEffectPool<T> {
    options: EffectPoolOptions,
    entries: VecDeque<PooledEffect<T>>,
    next_seed: i64,
}

impl<T> ParticleEffectPool<T> {
    /// An empty pool; a zero capacity is raised to one.
    pub fn new(options: EffectPoolOptions) -> Self {
        let options = EffectPoolOptions {
            capacity: options.capacity.max(1),
            ..options
        };
        Self {
            options,
            entries: VecDeque::with_capacity(options.capacity),
            next_seed: options.first_seed,
        }
    }

    /// Play `definition` at `position` (world units) with the next counter seed.
    ///
    /// The seed advances even when construction fails, so one failed cue does not shift every later effect.
    pub fn spawn(
        &mut self,
        definition: &CompiledParticleEffect,
        position: [f64; 3],
        data: T,
    ) -> Result<&mut PooledEffect<T>, ParticleEffectError> {
        let options = ParticleEffectOptions {
            seed: self.next_seed(),
            position,
            ..Default::default()
        };
        self.spawn_with(definition, options, data)
    }

    /// Play `definition` with complete options (a game-owned seed or random stream, or parameter overrides). Does not
    /// advance the pool's seed counter.
    pub fn spawn_with(
        &mut self,
        definition: &CompiledParticleEffect,
        options: ParticleEffectOptions,
        data: T,
    ) -> Result<&mut PooledEffect<T>, ParticleEffectError> {
        // Make room first, as the games this replaced did, so a full pool stays within budget even when the new
        // effect fails to construct.
        while self.entries.len() >= self.options.capacity {
            if let Some(mut evicted) = self.entries.pop_front() {
                evicted.effect.destroy();
            }
        }

        let effect = ParticleEffect::new(definition, options)?;
        self.entries.push_back(PooledEffect {
            effect,
            age_seconds: 0.0,
            data,
        });
        Ok(self.entries.back_mut().expect("the effect was just pushed"))
    }

    /// Take the next value of the pool's seed counter, for effects the owner keeps outside the pool (a looping torch)
    /// but wants on the same cosmetic sequence.
    pub fn next_seed(&mut self) -> i64 {
        let seed = self.next_seed;
        self.next_seed += 1;
        seed
    }

    /// Advance every effect by `delta_seconds` of real time (clamped to `[0, MAXIMUM_POOL_UPDATE_SECONDS]`), then drop
    /// finished and failed effects. Pass zero while paused: ages and particles hold.
    pub fn update(&mut self, delta_seconds: f64) {
        let delta_seconds =
            if delta_seconds.is_finite() { delta_seconds.clamp(0.0, MAXIMUM_POOL_UPDATE_SECONDS) } else { 0.0 };
        self.advance(delta_seconds, |effect| effect.update(delta_seconds));
    }

    /// Advance every effect by exactly one fixed particle step, for owners that drive effects from a fixed tick, then
    /// drop finished and failed effects.
    pub fn step(&mut self) {
        self.advance(crate::particle_effect::PARTICLE_STEP_SECONDS, ParticleEffect::step);
    }

    /// Live effects, oldest first; reverse it for newest first.
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, PooledEffect<T>> {
        self.entries.iter()
    }

    /// Live effects, oldest first, for owners that move or stop them.
    pub fn iter_mut(&mut self) -> std::collections::vec_deque::IterMut<'_, PooledEffect<T>> {
        self.entries.iter_mut()
    }

    /// Live effects only, oldest first; what a renderer draws.
    pub fn effects(&self) -> impl Iterator<Item = &ParticleEffect> {
        self.entries.iter().map(|entry| &entry.effect)
    }

    /// Number of live effects.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no effect is live.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Destroy every effect; the seed counter continues, so the next effects differ from the ones just cleared.
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            entry.effect.destroy();
        }
        self.entries.clear();
    }

    /// Destroy every effect and restart the seed counter, so a restarted level replays the same effects.
    pub fn restart(&mut self) {
        self.clear();
        self.next_seed = self.options.first_seed;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------------------------------------------------

    fn advance(
        &mut self,
        age_seconds: f64,
        mut advance: impl FnMut(&mut ParticleEffect) -> Result<(), ParticleEffectError>,
    ) {
        self.entries.retain_mut(|entry| {
            entry.age_seconds += age_seconds;
            if let Err(error) = advance(&mut entry.effect) {
                log::warn!("Particle effect stopped: {error}");
                entry.effect.destroy();
                return false;
            }
            if entry.effect.is_alive() {
                return true;
            }
            entry.effect.destroy();
            false
        });
    }
}

#[cfg(test)]
#[path = "particle_pool.test.rs"]
mod tests;
