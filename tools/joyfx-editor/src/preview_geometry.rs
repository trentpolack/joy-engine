// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Retained preview geometry, invalidated by the actual particles, definition, camera basis, or reference grid.
//! Comparing particle state also covers direct tool edits and resets at the same simulation time.

use joy_engine::particles::{
    CompiledParticleEffect,
    EffectParticle,
    ParticleBatch,
    ParticleCameraBasis,
    ParticleEffect,
    ParticleRenderError,
    ParticleRenderMetrics,
    create_particle_effect_batches,
};
use joy_engine::rendering::ColorVertex;

#[derive(Default)]
pub(crate) struct PreviewGeometry {
    pub batches: Vec<ParticleBatch>,
    definition: Option<CompiledParticleEffect>,
    particles: Vec<EffectParticle>,
    camera: Option<ParticleCameraBasis>,
    grid: Vec<ColorVertex>,
}

impl PreviewGeometry {
    /// Prepare the preview's triangles. Returns whether geometry was rebuilt.
    pub fn prepare(
        &mut self,
        effect: Option<&ParticleEffect>,
        camera: &ParticleCameraBasis,
        grid: &[ColorVertex],
        metrics: Option<&mut ParticleRenderMetrics>,
    ) -> Result<bool, ParticleRenderError> {
        let same_definition = match (&self.definition, effect) {
            (Some(previous), Some(effect)) => previous.ptr_eq(effect.definition()),
            (None, None) => true,
            _ => false,
        };
        let particles = effect.map(ParticleEffect::particles).unwrap_or_default();
        if same_definition && self.camera.as_ref() == Some(camera) && self.particles == particles && self.grid == grid {
            if let Some(metrics) = metrics {
                metrics.sort_ms = 0.0;
                for row in &mut metrics.emitters {
                    row.geometry_ms = 0.0;
                }
            }
            return Ok(false);
        }

        // Publish the new key only after a successful build; a failed build must be retried, never cached.
        self.camera = None;
        self.batches = match effect {
            Some(effect) => create_particle_effect_batches(effect, camera, metrics, grid)?,
            None if grid.is_empty() => Vec::new(),
            None => vec![ParticleBatch::Colored(grid.to_vec())],
        };
        self.definition = effect.map(|effect| effect.definition().clone());
        self.particles.clear();
        self.particles.extend_from_slice(particles);
        self.camera = Some(*camera);
        self.grid.clear();
        self.grid.extend_from_slice(grid);
        Ok(true)
    }

    pub fn clear(&mut self) {
        self.batches.clear();
        self.definition = None;
        self.particles.clear();
        self.camera = None;
        self.grid.clear();
    }
}

#[cfg(test)]
#[path = "preview_geometry.test.rs"]
mod tests;
