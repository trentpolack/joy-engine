// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Point lights emitted by particles of emitters with a `light` record.
//!
//! A particle light is an ordinary [`PointLight`] (with the emitter's shadow choice, off unless the effect opts in) plus
//! where it came from, so effects can rank their lights against each other before handing them to the renderer.

use crate::{
    MAX_POINT_LIGHTS,
    PointLight,
};
use joy_core::LinearColor;
use joy_core::math::Vec3;

use joy_fx::ParticleEffect;

/// A light emitted by one particle, in effect space (the effect's world units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticlePointLight {
    /// The light: position, linear HDR color (particle color × intensity × clamped alpha), range, and the emitter's
    /// `castsShadow` choice.
    pub light: PointLight,
    /// Emitting emitter.
    pub emitter_index: usize,
    /// Emitting particle's birth index (stable tie-breaker).
    pub particle_index: u64,
}

impl ParticlePointLight {
    /// Rec. 709 luminance of the emitted color, the ranking key.
    pub fn luminance(&self) -> f32 {
        self.light.luminance()
    }
}

/// The strongest particle lights and how many valid lights the cap excluded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleLightSelection {
    /// Lights ranked by luminance (brightest first), then birth index.
    pub lights: Vec<ParticlePointLight>,
    /// Valid emitting lights left out by the cap.
    pub omitted: usize,
}

impl ParticleLightSelection {
    /// The selected lights as renderer point lights.
    pub fn point_lights(&self) -> Vec<PointLight> {
        self.lights.iter().map(|light| light.light).collect()
    }
}

/// Collect an owned, bounded light list without mutating the simulation or consuming randomness.
///
/// Only live, visible particles of emitters with a `light` record whose intensity is in `(0, 1000]` and range in
/// `[0.01, 10000]` emit. `max_lights` above [`MAX_POINT_LIGHTS`] clamps to it. Colors are ranked in single precision,
/// the precision the renderer lights with.
pub fn collect_particle_lights(effect: &ParticleEffect, max_lights: usize) -> ParticleLightSelection {
    let limit = max_lights.min(MAX_POINT_LIGHTS);
    let emitters = effect.definition().emitters();

    let mut candidates = Vec::new();
    for particle in effect.particles() {
        let Some(emitter_light) = emitters.get(particle.emitter_index).and_then(|emitter| emitter.asset.light) else {
            continue;
        };
        if particle.age + 1e-10 >= particle.lifetime
            || particle.alpha <= 0.0
            || particle.light_intensity <= 0.0
            || particle.light_intensity > 1000.0
            || particle.light_range < 0.01
            || particle.light_range > 10000.0
        {
            continue;
        }
        let values = [
            particle.x,
            particle.y,
            particle.z,
            particle.r,
            particle.g,
            particle.b,
            particle.alpha,
            particle.light_intensity,
            particle.light_range,
        ];
        if !values.iter().all(|value| value.is_finite()) {
            continue;
        }
        let strength = particle.light_intensity * f64::min(1.0, particle.alpha);
        let r = f64::max(0.0, particle.r) * strength;
        let g = f64::max(0.0, particle.g) * strength;
        let b = f64::max(0.0, particle.b) * strength;
        if r + g + b <= 0.0 {
            continue;
        }
        candidates.push(ParticlePointLight {
            light: PointLight {
                position: Vec3::new(particle.x as f32, particle.y as f32, particle.z as f32),
                color: LinearColor::rgb(r as f32, g as f32, b as f32),
                range: particle.light_range as f32,
                casts_shadow: emitter_light.casts_shadow,
            },
            emitter_index: particle.emitter_index,
            particle_index: particle.index,
        });
    }
    rank_particle_lights(&mut candidates);
    let omitted = candidates.len().saturating_sub(limit);
    candidates.truncate(limit);
    ParticleLightSelection { lights: candidates, omitted }
}

/// Sort brightest first, breaking ties by birth index so equally bright particles keep a deterministic order.
#[doc(hidden)]
pub fn rank_particle_lights(lights: &mut [ParticlePointLight]) {
    lights.sort_by(|left, right| match right.luminance().partial_cmp(&left.luminance()) {
        Some(std::cmp::Ordering::Equal) | None => left.particle_index.cmp(&right.particle_index),
        Some(order) => order,
    });
}
