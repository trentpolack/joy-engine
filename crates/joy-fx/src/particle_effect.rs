// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One particle effect instance: fixed-step simulation, seeded cosmetic randomness, emission, and particle state.
//!
//! The simulation runs in a fixed step order so the same seed and inputs produce the same particles: real time
//! accumulates into [`PARTICLE_STEP_SECONDS`] ticks; each tick ages and updates survivors (update script, then
//! integration), queues trail and death children, retires expired particles, spawns the queued children, and finally
//! emits root emitters for the elapsed interval. Random numbers are drawn from one `SeededRandom` stream in a fixed
//! order, so seeded runs reproduce exactly on every target and match the golden recordings within their tolerances.
//!
//! Coordinates are XYZ world units; velocities and accelerations are per second; drag is an exponential damping
//! coefficient per second. Emission is cosmetic: keep effects off gameplay random streams.

use std::f64::consts::PI;

use joy_core::math::deterministic;
use joy_core::{
    RandomSource,
    SeededRandom,
};

use crate::asset::{
    CompiledEmitter,
    CompiledParticleEffect,
    ParticleParameterValue,
};
use crate::profile::{
    ParticleProfile,
    create_particle_profile,
    refresh_particle_profile_live,
    reset_particle_profile,
};
use crate::script::{
    FIXED_SLOT_COUNT,
    INPUT_SLOT_BASE,
    PARTICLE_FIELDS,
    ParticleScript,
    ParticleScriptError,
};

/// Fixed simulation step in seconds (60 Hz).
pub const PARTICLE_STEP_SECONDS: f64 = 1.0 / 60.0;

/// Deepest child generation that may still emit children.
const MAXIMUM_GENERATION: u32 = 4;

/// Lifetime spawn limit per effect run (until reset).
const MAXIMUM_TOTAL_SPAWNS: u64 = 65_536;

/// Script instruction units available to one step (shared by every spawn and update in it).
const MAXIMUM_STEP_OPERATIONS: i64 = 2_000_000;

/// Largest magnitude of particle fields and positions.
const MAXIMUM_POSITION: f64 = 1e6;

/// Tolerance used by every time comparison.
const TIME_EPSILON: f64 = 1e-10;

/// One live particle. Script-visible fields come first; the rest is private simulation bookkeeping.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectParticle {
    /// Position X in world units.
    pub x: f64,
    /// Position Y in world units.
    pub y: f64,
    /// Position Z in world units.
    pub z: f64,
    /// Velocity X in world units per second.
    pub vx: f64,
    /// Velocity Y in world units per second.
    pub vy: f64,
    /// Velocity Z in world units per second.
    pub vz: f64,
    /// Acceleration X in world units per second squared.
    pub ax: f64,
    /// Acceleration Y in world units per second squared.
    pub ay: f64,
    /// Acceleration Z in world units per second squared.
    pub az: f64,
    /// Exponential velocity damping per second, 0–1000.
    pub drag: f64,
    /// Radius (or half extent) in world units, 0–10000.
    pub size: f64,
    /// Billboard rotation in radians.
    pub rotation: f64,
    /// Rotation speed in radians per second.
    pub spin: f64,
    /// Linear HDR red.
    pub r: f64,
    /// Linear HDR green.
    pub g: f64,
    /// Linear HDR blue.
    pub b: f64,
    /// Straight alpha; values above one draw as one.
    pub alpha: f64,
    /// Point-light intensity multiplier, 0–1000.
    pub light_intensity: f64,
    /// Point-light range in world units, 0–10000.
    pub light_range: f64,
    /// Seconds until the particle expires, 0–30.
    pub lifetime: f64,
    /// Seconds lived.
    pub age: f64,
    /// Script scratch channel.
    pub u0: f64,
    /// Script scratch channel.
    pub u1: f64,
    /// Script scratch channel.
    pub u2: f64,
    /// Script scratch channel.
    pub u3: f64,
    /// Index of the emitter that spawned the particle.
    pub emitter_index: usize,
    /// Zero for root particles; children are one deeper than their parent.
    pub generation: u32,
    /// Birth order within the effect run (the script input `index`).
    pub index: u64,
    /// Spawn origin X (the script input `originX`).
    pub origin_x: f64,
    /// Spawn origin Y.
    pub origin_y: f64,
    /// Spawn origin Z.
    pub origin_z: f64,
    /// Parent velocity X at spawn (zero for roots).
    pub parent_vx: f64,
    /// Parent velocity Y at spawn.
    pub parent_vy: f64,
    /// Parent velocity Z at spawn.
    pub parent_vz: f64,
    /// Age at which the next trail child spawns (infinite without a trail).
    pub next_trail: f64,
}

impl EffectParticle {
    /// Script-visible fields in [`PARTICLE_FIELDS`] order.
    pub fn fields(&self) -> [f64; 24] {
        [
            self.x,
            self.y,
            self.z,
            self.vx,
            self.vy,
            self.vz,
            self.ax,
            self.ay,
            self.az,
            self.drag,
            self.size,
            self.rotation,
            self.spin,
            self.r,
            self.g,
            self.b,
            self.alpha,
            self.light_intensity,
            self.light_range,
            self.lifetime,
            self.u0,
            self.u1,
            self.u2,
            self.u3,
        ]
    }

    fn set_fields(&mut self, fields: &[f64]) {
        let field = |index: usize| fields.get(index).copied().unwrap_or(f64::NAN);
        self.x = field(0);
        self.y = field(1);
        self.z = field(2);
        self.vx = field(3);
        self.vy = field(4);
        self.vz = field(5);
        self.ax = field(6);
        self.ay = field(7);
        self.az = field(8);
        self.drag = field(9);
        self.size = field(10);
        self.rotation = field(11);
        self.spin = field(12);
        self.r = field(13);
        self.g = field(14);
        self.b = field(15);
        self.alpha = field(16);
        self.light_intensity = field(17);
        self.light_range = field(18);
        self.lifetime = field(19);
        self.u0 = field(20);
        self.u1 = field(21);
        self.u2 = field(22);
        self.u3 = field(23);
    }

    /// Whether the particle has not yet reached its lifetime.
    pub fn is_alive(&self) -> bool {
        self.age + TIME_EPSILON < self.lifetime
    }
}

/// Which script phase failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticlePhase {
    /// The spawn script.
    Spawn,
    /// The update script.
    Update,
}

impl std::fmt::Display for ParticlePhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Spawn => "spawn",
            Self::Update => "update",
        })
    }
}

/// A particle script or particle state failure.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ParticleFault {
    /// The script failed (non-finite result or exhausted budget) at a source location.
    #[error(transparent)]
    Script(#[from] ParticleScriptError),
    /// A script-visible field (named as in scripts, such as `lightIntensity`) became non-finite or exceeded ±1 million.
    #[error("Particle {0} must be finite and within ±1 million.")]
    FieldOutOfRange(&'static str),
    /// Light intensity, light range, size, drag, or lifetime left its supported range.
    #[error("Particle light intensity, light range, size, drag or lifetime exceeded its supported range.")]
    PropertyOutOfRange,
}

/// A particle effect could not be created or advanced. Messages are stable diagnostics.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ParticleEffectError {
    /// A seed given as a JSON number was not a finite integer (see [`particle_seed`]).
    #[error("Particle seed must be a finite integer.")]
    InvalidSeed,
    /// The origin had a non-finite component or one beyond ±1 million.
    #[error("Particle origin must be finite and within ±1 million world units.")]
    InvalidOrigin,
    /// A parameter override named an unknown parameter or had the wrong shape or range.
    #[error("Invalid value for effect parameter '{0}'.")]
    InvalidParameter(String),
    /// [`ParticleEffect::update`] received a delta outside `[0, 0.25]` seconds.
    #[error("Particle delta must be between 0 and 0.25 seconds.")]
    InvalidDelta,
    /// [`ParticleEffect::seek`] received a time outside `[0, 30]` seconds.
    #[error("Seek must be between 0 and 30 seconds.")]
    InvalidSeek,
    /// [`ParticleEffect::scale_positions`] received a non-positive factor or would exceed ±1 million.
    #[error("Scaled particle positions must be finite and within ±1 million.")]
    InvalidScale,
    /// The effect was destroyed.
    #[error("Particle effect has been destroyed.")]
    Destroyed,
    /// An emitter's script, or the particle state it produced, failed.
    #[error("Emitter '{emitter}' {phase}: {fault}")]
    Emitter {
        /// Emitter id.
        emitter: String,
        /// Script phase.
        phase: ParticlePhase,
        /// What failed.
        fault: ParticleFault,
    },
    /// Integration produced an invalid particle (reported without emitter context).
    #[error(transparent)]
    Simulation(ParticleFault),
}

/// Options for [`ParticleEffect::new`].
pub struct ParticleEffectOptions {
    /// Seed for the owned cosmetic stream, wrapped modulo 2^32 to an unsigned 32-bit value. Ignored when `random` is
    /// set.
    pub seed: i64,
    /// A game-owned random stream; reset and seek continue it instead of rewinding.
    pub random: Option<Box<dyn RandomSource>>,
    /// Parameter overrides by name; each must match its parameter's shape and bounds.
    pub parameters: Vec<(String, ParticleParameterValue)>,
    /// Spawn origin for root particles in world units; each component within ±1 million.
    pub position: [f64; 3],
    /// Collect CPU metrics in [`ParticleEffect::profile`].
    pub profiling: bool,
    /// Monotonic millisecond clock for profiling; defaults to a wall clock. Called only when profiling.
    pub profile_clock: Option<Box<dyn FnMut() -> f64>>,
}

impl Default for ParticleEffectOptions {
    fn default() -> Self {
        Self {
            seed: 0,
            random: None,
            parameters: Vec::new(),
            position: [0.0; 3],
            profiling: false,
            profile_clock: None,
        }
    }
}

/// Normalize an authored seed: it must be a finite whole number, and wraps modulo 2^32 so every whole number (negative
/// ones included) selects a stable `u32` random stream. Use it when seeds come from authored JSON.
pub fn particle_seed(value: f64) -> Result<u32, ParticleEffectError> {
    const SEED_MODULUS: f64 = 4_294_967_296.0;
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(ParticleEffectError::InvalidSeed);
    }

    // `rem_euclid` is exact for every whole `f64`, and its result is always in `[0, 2^32)`.
    Ok(value.rem_euclid(SEED_MODULUS) as u32)
}

/// Root emitter bookkeeping.
#[derive(Clone, Copy, Debug, Default)]
struct RootState {
    next_burst: usize,
    accumulator: f64,
}

/// The parent values a child spawn reads, captured when the child is queued.
#[derive(Clone, Copy, Debug)]
struct Parent {
    position: [f64; 3],
    velocity: [f64; 3],
    generation: u32,
}

/// A queued trail or death spawn.
#[derive(Clone, Copy, Debug)]
struct ChildRequest {
    emitter_index: usize,
    count: u64,
    parent: Parent,
}

/// Owns one effect's simulation, seed, parameters, and particles. The definition is shared and immutable.
pub struct ParticleEffect {
    // Shared definition and per-instance configuration.
    definition: CompiledParticleEffect,
    seed: u32,
    position: [f64; 3],
    parameter_values: Vec<f64>,

    // Cosmetic randomness: a borrowed game stream, or the owned seeded stream.
    borrowed_random: Option<Box<dyn RandomSource>>,
    owned_random: SeededRandom,

    // Simulation state.
    particles: Vec<EffectParticle>,
    ticks: u64,
    accumulator: f64,
    emitted: u64,
    dropped_particles: u64,
    stopped: bool,
    disposed: bool,
    error: Option<ParticleEffectError>,
    root_states: Vec<RootState>,
    budget: i64,
    scratch: Vec<f64>,

    // Diagnostics.
    profile: ParticleProfile,
    profile_clock: Box<dyn FnMut() -> f64>,
}

impl std::fmt::Debug for ParticleEffect {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParticleEffect")
            .field("name", &self.definition.asset().name)
            .field("seed", &self.seed)
            .field("ticks", &self.ticks)
            .field("particles", &self.particles.len())
            .field("stopped", &self.stopped)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl ParticleEffect {
    /// Create an instance and emit its time-zero bursts.
    ///
    /// Fails for an invalid origin or parameter override, or when a time-zero spawn script fails.
    pub fn new(
        definition: &CompiledParticleEffect,
        options: ParticleEffectOptions,
    ) -> Result<Self, ParticleEffectError> {
        let profile = create_particle_profile(definition, options.profiling);
        let seed = SeededRandom::new(options.seed).state();
        if options.position.iter().any(|value| !value.is_finite() || value.abs() > MAXIMUM_POSITION) {
            return Err(ParticleEffectError::InvalidOrigin);
        }

        // Defaults first, then validated overrides, flattened into script input order.
        let asset = definition.asset();
        let mut parameter_values: Vec<f64> =
            asset.parameters.values().flat_map(|parameter| parameter.default_value().components().to_vec()).collect();
        for (name, value) in &options.parameters {
            let Some(index) = asset.parameters.get_index_of(name) else {
                return Err(ParticleEffectError::InvalidParameter(name.clone()));
            };
            let parameter = &asset.parameters[index];
            if !parameter.accepts(value) {
                return Err(ParticleEffectError::InvalidParameter(name.clone()));
            }
            let offset: usize = asset
                .parameters
                .values()
                .take(index)
                .map(|parameter| parameter.default_value().components().len())
                .sum();
            parameter_values[offset..offset + value.components().len()].copy_from_slice(value.components());
        }

        let start = web_time::Instant::now();
        let profile_clock =
            options.profile_clock.unwrap_or_else(|| Box::new(move || start.elapsed().as_secs_f64() * 1000.0));
        let mut effect = Self {
            definition: definition.clone(),
            seed,
            position: options.position,
            parameter_values,
            borrowed_random: options.random,
            owned_random: SeededRandom::new(i64::from(seed)),
            particles: Vec::new(),
            ticks: 0,
            accumulator: 0.0,
            emitted: 0,
            dropped_particles: 0,
            stopped: false,
            disposed: false,
            error: None,
            root_states: vec![RootState::default(); definition.emitters().len()],
            budget: MAXIMUM_STEP_OPERATIONS,
            scratch: vec![f64::NAN; definition.slot_count().max(FIXED_SLOT_COUNT)],
            profile,
            profile_clock,
        };
        effect.reset()?;
        Ok(effect)
    }

    /// The shared definition.
    pub fn definition(&self) -> &CompiledParticleEffect {
        &self.definition
    }

    /// The normalized 32-bit seed of the owned stream.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// Root spawn origin in world units.
    pub fn position(&self) -> [f64; 3] {
        self.position
    }

    /// Live particles in birth order (oldest first).
    pub fn particles(&self) -> &[EffectParticle] {
        &self.particles
    }

    /// Mutable particles for tools and tests. Edits are validated by the next script run or integration.
    pub fn particles_mut(&mut self) -> &mut [EffectParticle] {
        &mut self.particles
    }

    /// Elapsed fixed-step simulation time in seconds; excludes the fractional accumulator.
    pub fn time(&self) -> f64 {
        (self.ticks as f64) * PARTICLE_STEP_SECONDS
    }

    /// Fixed steps since the last reset.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Accumulated real time not yet simulated, in seconds.
    pub fn accumulator(&self) -> f64 {
        self.accumulator
    }

    /// Particles spawned since the last reset.
    pub fn emitted(&self) -> u64 {
        self.emitted
    }

    /// Spawns rejected by the capacity or total-spawn limits since the last reset.
    pub fn dropped_particles(&self) -> u64 {
        self.dropped_particles
    }

    /// Whether emission has stopped (explicitly, after a failure, or after destruction).
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Whether [`destroy`](Self::destroy) was called.
    pub fn is_destroyed(&self) -> bool {
        self.disposed
    }

    /// The failure that stopped the simulation, if any; [`step`](Self::step) returns it until [`reset`](Self::reset).
    pub fn error(&self) -> Option<&ParticleEffectError> {
        self.error.as_ref()
    }

    /// Cumulative CPU metrics.
    pub fn profile(&self) -> &ParticleProfile {
        &self.profile
    }

    /// The current value of a flattened parameter component (`speed`, `windX`, `tint.r`).
    pub fn parameter(&self, name: &str) -> Option<f64> {
        let index = self.definition.parameter_inputs().iter().position(|input| input == name)?;
        self.parameter_values.get(index).copied()
    }

    /// Whether the effect still has live particles or pending root emission.
    pub fn is_alive(&self) -> bool {
        if self.disposed {
            return false;
        }
        if !self.particles.is_empty() {
            return true;
        }
        let time = self.time();
        !self.stopped
            && self.emitted < MAXIMUM_TOTAL_SPAWNS
            && self.definition.emitters().iter().zip(&self.root_states).any(|(emitter, state)| {
                let asset = &emitter.asset;
                asset.enabled
                    && asset.root
                    && (state.next_burst < asset.bursts.len() || (asset.rate > 0.0 && time < asset.duration))
            })
    }

    /// Accumulate real time and run every whole fixed step it covers, so frame cadence never changes the result.
    ///
    /// `delta_seconds` must be finite and within `[0, 0.25]`; owners clamp long pauses.
    pub fn update(&mut self, delta_seconds: f64) -> Result<(), ParticleEffectError> {
        self.assert_active()?;
        if !delta_seconds.is_finite() || !(0.0..=0.25).contains(&delta_seconds) {
            return Err(ParticleEffectError::InvalidDelta);
        }
        self.accumulator += delta_seconds;
        while self.accumulator + TIME_EPSILON >= PARTICLE_STEP_SECONDS {
            self.step()?;
            self.accumulator = f64::max(0.0, self.accumulator - PARTICLE_STEP_SECONDS);
        }
        Ok(())
    }

    /// Advance one fixed tick. Survivors update before newly emitted children.
    ///
    /// A failure stops emission and is stored; later steps return it until [`reset`](Self::reset).
    pub fn step(&mut self) -> Result<(), ParticleEffectError> {
        self.assert_active()?;
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let started = if self.profile.enabled { (self.profile_clock)() } else { 0.0 };
        self.budget = MAXIMUM_STEP_OPERATIONS;
        let previous_time = self.time();
        self.ticks += 1;

        let result = self.advance(previous_time).or_else(|error| self.fail(error));

        if self.profile.enabled {
            self.profile.steps += 1;
            refresh_particle_profile_live(&mut self.profile, &self.particles);
            let elapsed = (self.profile_clock)() - started;
            self.profile.simulation_ms += f64::max(0.0, elapsed);
        }
        result
    }

    /// Turn CPU profiling on or off without restarting the simulation. Enabling starts fresh totals (steps, timings,
    /// spawn and drop counts, operations) with live counts taken from the current particles; disabling keeps the last
    /// totals.
    pub fn set_profiling(&mut self, enabled: bool) {
        if self.profile.enabled == enabled {
            return;
        }
        self.profile.enabled = enabled;
        if enabled {
            reset_particle_profile(&mut self.profile);
            refresh_particle_profile_live(&mut self.profile, &self.particles);
        }
    }

    /// Reset time and particles at the current origin and emit time-zero bursts.
    ///
    /// The owned seeded stream rewinds; a borrowed stream continues from its current state.
    pub fn reset(&mut self) -> Result<(), ParticleEffectError> {
        self.assert_active()?;
        reset_particle_profile(&mut self.profile);
        let started = if self.profile.enabled { (self.profile_clock)() } else { 0.0 };
        self.particles.clear();
        self.owned_random = SeededRandom::new(i64::from(self.seed));
        self.ticks = 0;
        self.accumulator = 0.0;
        self.emitted = 0;
        self.dropped_particles = 0;
        self.stopped = false;
        self.error = None;
        self.root_states = vec![RootState::default(); self.definition.emitters().len()];
        self.budget = MAXIMUM_STEP_OPERATIONS;

        let result = self.emit_roots(0.0, 0.0).or_else(|error| self.fail(error));

        if self.profile.enabled {
            refresh_particle_profile_live(&mut self.profile, &self.particles);
            let elapsed = (self.profile_clock)() - started;
            self.profile.simulation_ms += f64::max(0.0, elapsed);
        }
        result
    }

    /// Replay to a fixed tick within 30 seconds; deterministic with the owned seed.
    ///
    /// Editors should split long seeks across frames when responsiveness matters.
    pub fn seek(&mut self, seconds: f64) -> Result<(), ParticleEffectError> {
        if !seconds.is_finite() || !(0.0..=30.0).contains(&seconds) {
            return Err(ParticleEffectError::InvalidSeek);
        }
        self.reset()?;
        let ticks = ((seconds / PARTICLE_STEP_SECONDS) + 1e-8).floor() as u64;
        for _ in 0..ticks {
            self.step()?;
        }
        Ok(())
    }

    /// Evict up to `count` oldest live particles for a session-wide cosmetic budget.
    ///
    /// Does not trigger death emitters or rewind the seed, clock, or total spawn count.
    pub fn remove_oldest_particles(&mut self, count: usize) -> Result<(), ParticleEffectError> {
        self.assert_active()?;
        let count = count.min(self.particles.len());
        self.particles.drain(..count);
        refresh_particle_profile_live(&mut self.profile, &self.particles);
        Ok(())
    }

    /// Rescale world positions about the origin after a world-bounds resize.
    ///
    /// Positions, spawn origins, and the effect origin scale per axis; velocities, sizes, time, and forces do not.
    /// Validation is atomic: nothing changes unless every factor is positive and finite and every scaled position stays
    /// within ±1 million.
    pub fn scale_positions(&mut self, scale: [f64; 3]) -> Result<(), ParticleEffectError> {
        self.assert_active()?;
        for (axis, factor) in scale.iter().copied().enumerate() {
            let exceeds = |value: f64| (value * factor).abs() > MAXIMUM_POSITION;
            let particles_exceed = self.particles.iter().any(|particle| {
                let (position, origin) = particle_axis(particle, axis);
                exceeds(position) || exceeds(origin)
            });
            if !factor.is_finite() || factor <= 0.0 || exceeds(self.position[axis]) || particles_exceed {
                return Err(ParticleEffectError::InvalidScale);
            }
        }
        for (position, factor) in self.position.iter_mut().zip(scale) {
            *position *= factor;
        }
        for particle in &mut self.particles {
            particle.x *= scale[0];
            particle.y *= scale[1];
            particle.z *= scale[2];
            particle.origin_x *= scale[0];
            particle.origin_y *= scale[1];
            particle.origin_z *= scale[2];
        }
        Ok(())
    }

    /// Stop root and child emission; live particles finish normally.
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    /// Idempotent terminal cleanup: particles are released and every later operation except reads fails.
    pub fn destroy(&mut self) {
        self.particles.clear();
        self.stopped = true;
        self.disposed = true;
        refresh_particle_profile_live(&mut self.profile, &self.particles);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Simulation internals
    // -----------------------------------------------------------------------------------------------------------------

    fn assert_active(&self) -> Result<(), ParticleEffectError> {
        if self.disposed {
            return Err(ParticleEffectError::Destroyed);
        }
        Ok(())
    }

    /// Record a failure, stop emission, and return it.
    fn fail(&mut self, error: ParticleEffectError) -> Result<(), ParticleEffectError> {
        self.error = Some(error.clone());
        self.stopped = true;
        Err(error)
    }

    /// The body of one step after the clock advanced: update survivors, spawn children, then emit roots.
    fn advance(&mut self, previous_time: f64) -> Result<(), ParticleEffectError> {
        let time = self.time();
        let mut children = Vec::new();
        let mut alive = Vec::with_capacity(self.particles.len());

        {
            let Self {
                definition,
                parameter_values,
                borrowed_random,
                owned_random,
                particles,
                stopped,
                budget,
                scratch,
                profile,
                profile_clock,
                ..
            } = self;
            let random: &mut dyn RandomSource = match borrowed_random {
                Some(random) => random.as_mut(),
                None => owned_random,
            };
            let mut runner = ScriptRunner {
                parameter_values,
                random,
                budget,
                scratch,
                profile,
                time,
            };

            for particle in particles.iter_mut() {
                let emitter_index = particle.emitter_index;
                let Some(emitter) = definition.emitters().get(emitter_index) else {
                    alive.push(false);
                    continue;
                };
                let timed = runner.profile.enabled;
                let update_started = if timed { profile_clock() } else { 0.0 };
                let outcome = update_particle(&mut runner, emitter, particle, *stopped, &mut children);
                if timed && let Some(row) = runner.profile.emitters.get_mut(emitter_index) {
                    row.update_ms += f64::max(0.0, profile_clock() - update_started);
                }
                alive.push(outcome?);
            }
        }

        // Retire expired particles only after every update succeeded; a failed update leaves them in the list.
        let mut flags = alive.into_iter();
        self.particles.retain(|_| flags.next().unwrap_or(false));
        if !self.stopped {
            for request in children {
                self.spawn(request.emitter_index, request.count, Some(request.parent))?;
            }
            self.emit_roots(previous_time, time)?;
        }
        Ok(())
    }

    /// Emit root emitters over `[start, end]` seconds, including unconsumed bursts at its end.
    fn emit_roots(&mut self, start: f64, end: f64) -> Result<(), ParticleEffectError> {
        for index in 0..self.definition.emitters().len() {
            let emitter = &self.definition.emitters()[index].asset;
            if !emitter.enabled || !emitter.root {
                continue;
            }
            let state = &mut self.root_states[index];
            let active_seconds = f64::max(0.0, f64::min(end, emitter.duration) - f64::min(start, emitter.duration));
            state.accumulator += emitter.rate * active_seconds;
            let mut count = (state.accumulator + TIME_EPSILON).floor();
            state.accumulator = f64::max(0.0, state.accumulator - count);
            while state.next_burst < emitter.bursts.len() && emitter.bursts[state.next_burst].time <= end + TIME_EPSILON
            {
                count += f64::from(emitter.bursts[state.next_burst].count);
                state.next_burst += 1;
            }
            self.spawn(index, count as u64, None)?;
        }
        Ok(())
    }

    /// Spawn up to `count` particles of an emitter, at the parent (children) or the effect origin (roots).
    fn spawn(&mut self, emitter_index: usize, count: u64, parent: Option<Parent>) -> Result<(), ParticleEffectError> {
        let Self {
            definition,
            position,
            parameter_values,
            borrowed_random,
            owned_random,
            particles,
            emitted,
            dropped_particles,
            budget,
            scratch,
            profile,
            profile_clock,
            ..
        } = self;
        let Some(emitter) = definition.emitters().get(emitter_index) else {
            return Ok(());
        };
        if !emitter.asset.enabled || count == 0 {
            return Ok(());
        }
        let time = (self.ticks as f64) * PARTICLE_STEP_SECONDS;
        let random: &mut dyn RandomSource = match borrowed_random {
            Some(random) => random.as_mut(),
            None => owned_random,
        };
        let timed = profile.enabled;
        let started = if timed { profile_clock() } else { 0.0 };
        let mut runner = ScriptRunner {
            parameter_values,
            random,
            budget,
            scratch,
            profile,
            time,
        };

        let capacity = i64::from(definition.asset().max_particles) - particles.len() as i64;
        let remaining_spawns = MAXIMUM_TOTAL_SPAWNS as i64 - *emitted as i64;
        let available = capacity.min(remaining_spawns).max(0) as u64;
        let accepted = count.min(available);
        *dropped_particles += count - accepted;
        if timed && let Some(row) = runner.profile.emitters.get_mut(emitter_index) {
            row.dropped += count - accepted;
        }

        let mut result = Ok(());
        for _ in 0..accepted {
            let mut particle = new_particle(emitter, emitter_index, *position, parent, *emitted);
            *emitted += 1;
            if timed && let Some(row) = runner.profile.emitters.get_mut(emitter_index) {
                row.spawned += 1;
            }
            if let Err(error) = runner.execute(emitter, &emitter.spawn_program, &mut particle, ParticlePhase::Spawn) {
                result = Err(error);
                break;
            }
            particles.push(particle);
        }

        if timed && let Some(row) = runner.profile.emitters.get_mut(emitter_index) {
            row.spawn_ms += f64::max(0.0, profile_clock() - started);
        }
        result
    }
}

/// Split borrows needed to run one script against one particle.
struct ScriptRunner<'e> {
    parameter_values: &'e [f64],
    random: &'e mut dyn RandomSource,
    budget: &'e mut i64,
    scratch: &'e mut Vec<f64>,
    profile: &'e mut ParticleProfile,
    time: f64,
}

impl ScriptRunner<'_> {
    /// Run `program` for `particle`: load fields, inputs, and parameters, evaluate, store fields, then validate.
    fn execute(
        &mut self,
        emitter: &CompiledEmitter,
        program: &ParticleScript,
        particle: &mut EffectParticle,
        phase: ParticlePhase,
    ) -> Result<(), ParticleEffectError> {
        if self.scratch.len() < program.slot_count() {
            self.scratch.resize(program.slot_count(), f64::NAN);
        }
        let slots = self.scratch.as_mut_slice();
        slots[..PARTICLE_FIELDS.len()].copy_from_slice(&particle.fields());
        let inputs = [
            PARTICLE_STEP_SECONDS,
            particle.age,
            f64::min(1.0, particle.age / f64::max(1e-10, particle.lifetime)),
            particle.index as f64,
            self.time,
            particle.origin_x,
            particle.origin_y,
            particle.origin_z,
            particle.parent_vx,
            particle.parent_vy,
            particle.parent_vz,
            PI,
            PI * 2.0,
        ];
        slots[INPUT_SLOT_BASE..FIXED_SLOT_COUNT].copy_from_slice(&inputs);
        for (value, slot) in self.parameter_values.iter().zip(program.parameter_slots()) {
            slots[*slot] = *value;
        }

        let before = *self.budget;
        let result = program.execute(slots, self.random, self.budget).map_err(ParticleFault::Script).and_then(|()| {
            particle.set_fields(&slots[..PARTICLE_FIELDS.len()]);
            validate_particle(particle)
        });
        if self.profile.enabled
            && let Some(row) = self.profile.emitters.get_mut(particle.emitter_index)
        {
            row.operations += before - *self.budget;
        }
        result.map_err(|fault| ParticleEffectError::Emitter {
            emitter: emitter.asset.id.clone(),
            phase,
            fault,
        })
    }
}

/// Age, update, and integrate one particle, queueing its trail or death children; returns whether it survives.
fn update_particle(
    runner: &mut ScriptRunner<'_>,
    emitter: &CompiledEmitter,
    particle: &mut EffectParticle,
    stopped: bool,
    children: &mut Vec<ChildRequest>,
) -> Result<bool, ParticleEffectError> {
    particle.age += PARTICLE_STEP_SECONDS;
    runner.execute(emitter, &emitter.update_program, particle, ParticlePhase::Update)?;
    integrate(particle).map_err(ParticleEffectError::Simulation)?;
    let alive = particle.is_alive();
    if !stopped && particle.generation < MAXIMUM_GENERATION {
        let parent = Parent {
            position: [particle.x, particle.y, particle.z],
            velocity: [particle.vx, particle.vy, particle.vz],
            generation: particle.generation,
        };
        if let (true, Some(trail), Some(index)) = (alive, &emitter.asset.trail, emitter.trail_index)
            && particle.age + TIME_EPSILON >= particle.next_trail
        {
            particle.next_trail += trail.interval;
            children.push(ChildRequest {
                emitter_index: index,
                count: u64::from(trail.count),
                parent,
            });
        }
        if let (false, Some(death), Some(index)) = (alive, &emitter.asset.death, emitter.death_index) {
            children.push(ChildRequest {
                emitter_index: index,
                count: u64::from(death.count),
                parent,
            });
        }
    }
    Ok(alive)
}

/// A new particle with engine defaults, before its spawn script runs.
fn new_particle(
    emitter: &CompiledEmitter,
    emitter_index: usize,
    position: [f64; 3],
    parent: Option<Parent>,
    index: u64,
) -> EffectParticle {
    let origin = parent.map_or(position, |parent| parent.position);
    let parent_velocity = parent.map_or([0.0; 3], |parent| parent.velocity);
    let light = emitter.asset.light;
    EffectParticle {
        x: origin[0],
        y: origin[1],
        z: origin[2],
        vx: 0.0,
        vy: 0.0,
        vz: 0.0,
        ax: 0.0,
        ay: 0.0,
        az: 0.0,
        drag: 0.0,
        size: 4.0,
        rotation: 0.0,
        spin: 0.0,
        r: 2.0,
        g: 0.5,
        b: 0.1,
        alpha: 1.0,
        light_intensity: light.map_or(0.0, |light| light.intensity),
        light_range: light.map_or(0.0, |light| light.range),
        lifetime: emitter.asset.lifetime,
        age: 0.0,
        u0: 0.0,
        u1: 0.0,
        u2: 0.0,
        u3: 0.0,
        emitter_index,
        generation: parent.map_or(0, |parent| parent.generation + 1),
        index,
        origin_x: origin[0],
        origin_y: origin[1],
        origin_z: origin[2],
        parent_vx: parent_velocity[0],
        parent_vy: parent_velocity[1],
        parent_vz: parent_velocity[2],
        next_trail: emitter.asset.trail.as_ref().map_or(f64::INFINITY, |trail| trail.interval),
    }
}

/// Integrate velocity before position, then damp velocity for the next tick.
fn integrate(particle: &mut EffectParticle) -> Result<(), ParticleFault> {
    let dt = PARTICLE_STEP_SECONDS;
    particle.vx += particle.ax * dt;
    particle.vy += particle.ay * dt;
    particle.vz += particle.az * dt;
    particle.x += particle.vx * dt;
    particle.y += particle.vy * dt;
    particle.z += particle.vz * dt;
    let damping = deterministic::exp(-particle.drag * dt);
    particle.vx *= damping;
    particle.vy *= damping;
    particle.vz *= damping;
    particle.rotation += particle.spin * dt;
    validate_particle(particle)
}

/// Reject particle state the renderer and later steps cannot use safely.
fn validate_particle(particle: &EffectParticle) -> Result<(), ParticleFault> {
    for (name, value) in PARTICLE_FIELDS.iter().zip(particle.fields()) {
        if !value.is_finite() || value.abs() > MAXIMUM_POSITION {
            return Err(ParticleFault::FieldOutOfRange(name));
        }
    }
    let outside = |value: f64, max: f64| !(0.0..=max).contains(&value);
    if outside(particle.light_intensity, 1000.0)
        || outside(particle.light_range, 10000.0)
        || outside(particle.size, 10000.0)
        || outside(particle.drag, 1000.0)
        || outside(particle.lifetime, 30.0)
    {
        return Err(ParticleFault::PropertyOutOfRange);
    }
    Ok(())
}

/// A particle's position and spawn origin along one axis.
fn particle_axis(particle: &EffectParticle, axis: usize) -> (f64, f64) {
    match axis {
        0 => (particle.x, particle.origin_x),
        1 => (particle.y, particle.origin_y),
        _ => (particle.z, particle.origin_z),
    }
}
