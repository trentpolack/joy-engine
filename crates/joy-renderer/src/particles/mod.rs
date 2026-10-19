// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle drawing: turns `joy_fx` effects into renderer point lights and camera-facing triangles for the transparent
//! queue (`FrameInputs::transparent_triangles` or `FrameInputs::transparent_draws`), and owns the decoded textures they
//! sample. Simulation, documents, and scripts stay in `joy_fx`; `joy_engine::particles` combines both.
//!
//! [`collect_particle_lights`] turns particles into point lights; [`append_particle_effect`] or
//! [`create_particle_effect_batches`] build triangles, using the same deterministic `f64` math as the simulation so
//! geometry reproduces on every target.
//!
//! ```
//! use joy_fx::{ParticleEffect, ParticleEffectOptions, compile_particle_effect};
//! use joy_renderer::particles::{PARTICLE_XY_BASIS, create_particle_effect_batches};
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
//! let batches = create_particle_effect_batches(&effect, &PARTICLE_XY_BASIS, None, &[])?;
//! assert_eq!(batches.len(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Ownership
//!
//! Geometry builders borrow effects read-only and return owned vertices. [`ParticleTextureSet`] owns CPU images; the
//! renderer owns their GPU copies.

mod batches;
mod lights;
mod texture_set;

pub use batches::{
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
pub use lights::{
    ParticleLightSelection,
    ParticlePointLight,
    collect_particle_lights,
};
pub use texture_set::{
    ParticleTextureSet,
    decode_particle_texture,
};

// Geometry, light, and texture helpers shared with `joy-scene`'s `AssetScene`, which sorts and lights many effects
// together; an engine contract between the two crates rather than game-facing API.
#[doc(hidden)]
pub use batches::{
    ParticleCorner,
    ParticleTriangle,
    batch_particle_triangles,
    collect_particle_triangles,
};
#[doc(hidden)]
pub use lights::rank_particle_lights;
#[doc(hidden)]
pub use texture_set::textured_sources;
