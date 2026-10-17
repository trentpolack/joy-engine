// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Point lights emitted by particles of emitters with a `light` record.

use crate::rendering::{
    MAX_POINT_LIGHTS,
    PointLight,
};
use glam::Vec3;

use crate::particles::math;
use crate::particles::particle_effect::ParticleEffect;

/// A particle light in effect space (the effect's world units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticlePointLight {
    /// Position X.
    pub x: f64,
    /// Position Y.
    pub y: f64,
    /// Position Z.
    pub z: f64,
    /// Linear HDR red: particle red × intensity × clamped alpha.
    pub r: f64,
    /// Linear HDR green.
    pub g: f64,
    /// Linear HDR blue.
    pub b: f64,
    /// Range in world units.
    pub range: f64,
    /// Emitting emitter.
    pub emitter_index: usize,
    /// Emitting particle's birth index (stable tie-breaker).
    pub particle_index: u64,
}

impl ParticlePointLight {
    /// Rec. 709 luminance of the emitted color, the ranking key.
    pub fn luminance(&self) -> f64 {
        (self.r * 0.2126) + (self.g * 0.7152) + (self.b * 0.0722)
    }

    /// The renderer's point light (single precision, no shadow).
    pub fn point_light(&self) -> PointLight {
        PointLight {
            position: Vec3::new(self.x as f32, self.y as f32, self.z as f32),
            color: [self.r as f32, self.g as f32, self.b as f32],
            range: self.range as f32,
            casts_shadow: false,
        }
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
        self.lights.iter().map(ParticlePointLight::point_light).collect()
    }
}

/// Collect an owned, bounded light list without mutating the simulation or consuming randomness.
///
/// Only live, visible particles of emitters with a `light` record whose intensity is in `(0, 1000]` and range in
/// `[0.01, 10000]` emit. `max_lights` above [`MAX_POINT_LIGHTS`] clamps to it.
pub fn collect_particle_lights(effect: &ParticleEffect, max_lights: usize) -> ParticleLightSelection {
    let limit = max_lights.min(MAX_POINT_LIGHTS);
    let emitters = effect.definition().emitters();

    let mut candidates = Vec::new();
    for particle in effect.particles() {
        let has_light = emitters.get(particle.emitter_index).is_some_and(|emitter| emitter.asset.light.is_some());
        if !has_light
            || particle.age + 1e-10 >= particle.lifetime
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
        let strength = particle.light_intensity * math::min(1.0, particle.alpha);
        let r = math::max(0.0, particle.r) * strength;
        let g = math::max(0.0, particle.g) * strength;
        let b = math::max(0.0, particle.b) * strength;
        if r + g + b <= 0.0 {
            continue;
        }
        candidates.push(ParticlePointLight {
            x: particle.x,
            y: particle.y,
            z: particle.z,
            r,
            g,
            b,
            range: particle.light_range,
            emitter_index: particle.emitter_index,
            particle_index: particle.index,
        });
    }
    rank_particle_lights(&mut candidates);
    let omitted = candidates.len().saturating_sub(limit);
    candidates.truncate(limit);
    ParticleLightSelection { lights: candidates, omitted }
}

/// Sort brightest first, breaking ties by birth index, like the TypeScript comparator.
pub(crate) fn rank_particle_lights(lights: &mut [ParticlePointLight]) {
    lights.sort_by(|left, right| match right.luminance().partial_cmp(&left.luminance()) {
        Some(std::cmp::Ordering::Equal) | None => left.particle_index.cmp(&right.particle_index),
        Some(order) => order,
    });
}
