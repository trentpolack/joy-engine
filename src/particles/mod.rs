// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle effects (`.joyfx`): documents, the particle script language, deterministic simulation, lights, and
//! renderer geometry in the rendering layer.
//!
//! Data flow: a `.joyfx` document compiles once into a shared [`CompiledParticleEffect`]
//! ([`compile_particle_effect`]); each [`ParticleEffect`] instance owns its particles, seed, and parameters and advances
//! in fixed [`PARTICLE_STEP_SECONDS`] ticks; [`collect_particle_lights`] turns particles into renderer point lights; and
//! [`append_particle_effect`] or [`create_particle_effect_batches`] build camera-facing triangles for the renderer's
//! transparent queue (`FrameInputs::transparent_triangles` or `FrameInputs::transparent_draws`).
//!
//! ```
//! use joy_engine::particles::{
//!     PARTICLE_XY_BASIS, ParticleEffect, ParticleEffectOptions, compile_particle_effect, create_particle_effect_batches,
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
//! let batches = create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &[])?;
//! assert_eq!(batches.len(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Determinism
//!
//! Simulation uses `f64`, consumes the seeded cosmetic stream ([`crate::core::SeededRandom`])
//! in the same order, and computes transcendental functions with the portable `libm` crate, so native and WebAssembly
//! builds agree exactly. Parity tests compare against values recorded from the retired TypeScript engine; V8's `sin`, `cos`,
//! `log`, and `pow` occasionally differ in the last bit (see `docs/rust-particles.md`).
//!
//! # Ownership
//!
//! Definitions are immutable and shared. Effects own their particles and borrow nothing from the caller after
//! construction. Geometry builders borrow effects read-only and return owned vertices. [`ParticleTextureSet`] owns CPU
//! images; the renderer owns their GPU copies.

mod asset;
mod constants;
mod language_information;
mod lights;
mod math;
mod particle_effect;
mod profile;
mod rendering;
mod script;
pub mod scripting;
mod texture_set;
mod texture_source;

pub use asset::{
    CompiledEmitter,
    CompiledParticleEffect,
    MAXIMUM_EFFECT_PARTICLES,
    PARTICLE_EFFECT_VERSION,
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
pub use lights::{
    ParticleLightSelection,
    ParticlePointLight,
    collect_particle_lights,
};
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
pub use rendering::{
    PARTICLE_XY_BASIS,
    ParticleBatch,
    ParticleCameraBasis,
    ParticleRenderError,
    ParticleRenderMetrics,
    ParticleRenderMetricsRow,
    append_particle_effect,
    create_particle_effect_batches,
    create_particle_render_metrics,
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
pub use texture_set::{
    ParticleTextureSet,
    decode_particle_texture,
};
pub use texture_source::{
    DEFAULT_PARTICLE_TEXTURE,
    MAXIMUM_PARTICLE_TEXTURE_SIZE,
    ParticleTextureDimensions,
    ParticleTextureError,
    encode_particle_texture_source,
    read_particle_texture_source,
};

// Crate-internal geometry and light helpers shared with `crate::rendering::AssetScene`.
pub(crate) use lights::rank_particle_lights;
pub(crate) use rendering::{
    ParticleTriangle,
    batch_particle_triangles,
    collect_particle_triangles,
};
pub(crate) use texture_set::textured_sources;
