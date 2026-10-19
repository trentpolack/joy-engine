// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle effects (`.joyfx`): documents, the particle script language, and deterministic simulation. Renderer-free:
//! drawing lives in the renderer's `particles` module, which turns effects into camera-facing geometry and lights
//! (`joy_engine::particles` combines both for games and tools).
//!
//! Data flow: a `.joyfx` document compiles once into a shared [`CompiledParticleEffect`]
//! ([`compile_particle_effect`]); each [`ParticleEffect`] instance owns its particles, seed, and parameters and advances
//! in fixed [`PARTICLE_STEP_SECONDS`] ticks; the renderer reads the particles to build lights and triangles for its
//! transparent queue.
//!
//! ```
//! use joy_fx::{
//!     ParticleEffect, ParticleEffectOptions, compile_particle_effect,
//! };
//! use serde_json::json;
//!
//! let definition = compile_particle_effect(&json!({
//!     "version": 1, "name": "Sparks", "maxParticles": 32, "parameters": {"speed": {"value": 4, "min": 0, "max": 10}},
//!     "emitters": [{"id": "sparks", "enabled": true, "root": true, "duration": 0, "rate": 0,
//!         "bursts": [{"time": 0, "count": 8}], "lifetime": 1, "renderer": "soft",
//!         "spawn": "velocity = vector(rand(-1, 1), 1, 0) * speed; size = rand(0.1, 0.3);",
//!         "update": "color.a = 1 - t;"}]
//! }))?;
//! let mut effect = ParticleEffect::new(&definition, ParticleEffectOptions { seed: 7, ..Default::default() })?;
//! effect.update(0.1)?;
//! assert_eq!(effect.particles().len(), 8);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Determinism
//!
//! Simulation uses `f64`, consumes the seeded cosmetic stream ([`joy_core::SeededRandom`])
//! in the same order, and computes transcendental functions with the portable `libm` crate, so native and WebAssembly
//! builds agree exactly. Parity tests compare against values recorded from the retired TypeScript engine; V8's `sin`, `cos`,
//! `log`, and `pow` occasionally differ in the last bit (see `docs/rust-particles.md`).
//!
//! # Ownership
//!
//! Definitions are immutable and shared. Effects own their particles and borrow nothing from the caller after
//! construction; the renderer's geometry builders borrow effects read-only.

mod asset;
mod constants;
mod language_information;
mod particle_effect;
mod profile;
mod script;
pub mod scripting;
mod texture_source;

pub use asset::{
    CompiledEmitter,
    CompiledParticleEffect,
    MAXIMUM_EFFECT_PARTICLES,
    PARTICLE_EFFECT_VERSION,
    PARTICLE_LIGHT_CASTS_SHADOW,
    ParticleAssetError,
    ParticleBurst,
    ParticleChild,
    ParticleEffectAsset,
    ParticleEmitterAsset,
    ParticleLight,
    ParticleParameter,
    ParticleParameterValue,
    ParticleTexture,
    ParticleTrail,
    compile_particle_effect,
    parse_particle_effect,
};
pub use constants::{
    ParticleParameterType,
    ParticleRenderer,
    ParticleValueKind,
};
pub use language_information::particle_language_catalog;
pub use particle_effect::{
    EffectParticle,
    PARTICLE_STEP_SECONDS,
    ParticleEffect,
    ParticleEffectError,
    ParticleEffectOptions,
    ParticleFault,
    ParticlePhase,
    particle_seed,
};
pub use profile::{
    ParticleProfile,
    ParticleProfileRow,
    create_particle_profile,
    refresh_particle_profile_live,
    reset_particle_profile,
};
pub use script::{
    MAXIMUM_SCRIPT_LENGTH,
    MAXIMUM_SCRIPT_TOKENS,
    PARTICLE_CONTAINERS,
    PARTICLE_FIELDS,
    PARTICLE_FUNCTION_ARITIES,
    PARTICLE_INPUTS,
    ParticleScript,
    ParticleScriptBudget,
    ParticleScriptError,
    compile_particle_script,
};
pub use texture_source::{
    DEFAULT_PARTICLE_TEXTURE,
    MAXIMUM_PARTICLE_TEXTURE_SIZE,
    ParticleTextureDimensions,
    ParticleTextureError,
    decode_particle_texture_source,
    encode_particle_texture_source,
    read_particle_texture_source,
};
