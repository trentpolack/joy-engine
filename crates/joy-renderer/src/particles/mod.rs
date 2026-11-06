// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle drawing: turns `joy_fx` effects into renderer point lights and GPU-instanced particles, and owns the
//! decoded textures they sample. Simulation, documents, and scripts stay in `joy_fx`; `joy_engine::particles` combines
//! both.
//!
//! [`collect_particle_lights`] turns particles into point lights. [`ParticleFrame`] is the one way to submit particles
//! for drawing: add every effect each frame, then pass the frame as
//! [`FrameInputs::particles`](crate::FrameInputs::particles) (or
//! [`TriangleFrame::particles`](crate::TriangleFrame::particles) for arcade triangle frames). Each particle becomes one
//! compact [`ParticleInstance`]; the shaders expand it to face the camera and draw its shape analytically, and the
//! renderer culls whole batches, sorts source-over batches back to front, and blends each emitter with its authored
//! [`ParticleBlend`](joy_fx::ParticleBlend).
//!
//! ```
//! use joy_fx::{ParticleEffect, ParticleEffectOptions, compile_particle_effect};
//! use joy_renderer::particles::ParticleFrame;
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
//! let mut particles = ParticleFrame::new();
//! particles.add_effect(&effect, None)?;
//! assert_eq!((particles.instances().len(), particles.batches().len()), (8, 1));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Ownership
//!
//! A [`ParticleFrame`] borrows effects only while they are added and keeps cheap texture handles until it is cleared.
//! [`ParticleTextureSet`] owns CPU images; the renderer owns their GPU copies.

mod frame;
mod instance;
mod lights;
mod texture_set;

pub use frame::{
    ParticleBatch,
    ParticleFrame,
    ParticleRenderError,
};
pub use instance::ParticleInstance;
pub use lights::{
    ParticleLightSelection,
    ParticlePointLight,
    collect_particle_lights,
};
pub use texture_set::{
    ParticleTextureSet,
    decode_particle_texture,
};

// Light and texture helpers shared with `joy-scene`'s `AssetScene`, which lights many effects together and stages their
// textures; an engine contract between the two crates rather than game-facing API.
#[doc(hidden)]
pub use lights::rank_particle_lights;
#[doc(hidden)]
pub use texture_set::textured_sources;
