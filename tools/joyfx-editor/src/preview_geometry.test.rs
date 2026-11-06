// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Cached previews must invalidate for authoring edits, playback, camera changes, and grid changes.

use super::*;
use joy_engine::particles::{
    PARTICLE_XY_BASIS,
    ParticleEffectOptions,
    compile_particle_effect,
    create_particle_render_metrics,
};

fn ember() -> ParticleEffect {
    let definition = compile_particle_effect(&crate::examples::example_json(0)).unwrap();
    ParticleEffect::new(
        &definition,
        ParticleEffectOptions {
            seed: 777,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn unchanged_particles_reuse_geometry_and_clear_last_frame_timings() {
    let mut effect = ember();
    for _ in 0..41 {
        effect.step().unwrap();
    }
    let mut geometry = PreviewGeometry::default();
    let mut metrics = create_particle_render_metrics(effect.definition());
    assert!(geometry.prepare(Some(&effect), &PARTICLE_XY_BASIS, &[], Some(&mut metrics)).unwrap());
    let before = geometry.batches.clone();
    let counts: Vec<_> = metrics.emitters.iter().map(|row| (row.vertices, row.triangles)).collect();
    metrics.sort_ms = 123.0;
    for row in &mut metrics.emitters {
        row.geometry_ms = 123.0;
    }
    assert!(!geometry.prepare(Some(&effect), &PARTICLE_XY_BASIS, &[], Some(&mut metrics)).unwrap());
    assert_eq!(geometry.batches, before);
    assert_eq!(metrics.sort_ms, 0.0);
    assert!(metrics.emitters.iter().all(|row| row.geometry_ms == 0.0));
    assert_eq!(metrics.emitters.iter().map(|row| (row.vertices, row.triangles)).collect::<Vec<_>>(), counts);
}

#[test]
fn edits_playback_and_definition_replacement_invalidate_the_preview() {
    let mut effect = ember();
    let mut geometry = PreviewGeometry::default();
    let mut prepare = |effect: &ParticleEffect| {
        assert!(geometry.prepare(Some(effect), &PARTICLE_XY_BASIS, &[], None).unwrap());
        assert_eq!(geometry.batches, create_particle_effect_batches(effect, &PARTICLE_XY_BASIS, None, &[]).unwrap());
        assert!(!geometry.prepare(Some(effect), &PARTICLE_XY_BASIS, &[], None).unwrap());
    };
    prepare(&effect);
    effect.particles_mut()[0].size *= 2.0;
    prepare(&effect);
    effect.particles_mut()[0].alpha = 0.0;
    prepare(&effect);
    effect.step().unwrap();
    prepare(&effect);
    effect.reset().unwrap();
    prepare(&effect);
    // Identical simulation time and particles do not establish that the renderer definition is unchanged.
    let mut document = crate::examples::example_json(0);
    document["emitters"][0]["renderer"] = joy_engine::serde_json::json!("square");
    let definition = compile_particle_effect(&document).unwrap();
    let replacement = ParticleEffect::new(
        &definition,
        ParticleEffectOptions {
            seed: 777,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(effect.particles(), replacement.particles());
    prepare(&replacement);
}

#[test]
fn camera_grid_and_lifecycle_changes_invalidate_without_caching_errors() {
    let effect = ember();
    let mut geometry = PreviewGeometry::default();
    assert!(geometry.prepare(Some(&effect), &PARTICLE_XY_BASIS, &[], None).unwrap());
    let camera = ParticleCameraBasis {
        right: [0.0, 0.0, -1.0],
        up: [0.0, 1.0, 0.0],
        backward: [1.0, 0.0, 0.0],
    };
    assert!(geometry.prepare(Some(&effect), &camera, &[], None).unwrap());
    let grid = [ColorVertex {
        position: [0.0, 0.0, 0.0],
        color: [0.5; 4],
    }; 3];
    assert!(geometry.prepare(Some(&effect), &camera, &grid, None).unwrap());
    assert_eq!(geometry.batches, create_particle_effect_batches(&effect, &camera, None, &grid).unwrap());
    assert!(!geometry.prepare(Some(&effect), &camera, &grid, None).unwrap());
    for _ in 0..2 {
        assert_eq!(
            geometry.prepare(Some(&effect), &camera, &grid[..2], None),
            Err(ParticleRenderError::IncompleteTriangles)
        );
    }
    assert!(geometry.prepare(Some(&effect), &camera, &[], None).unwrap());
    geometry.clear();
    assert!(geometry.batches.is_empty());
    assert!(geometry.prepare(Some(&effect), &camera, &[], None).unwrap());
    assert!(geometry.prepare(None, &camera, &grid, None).unwrap());
    assert_eq!(geometry.batches, vec![ParticleBatch::Colored(grid.to_vec())]);
    assert!(geometry.prepare(None, &camera, &[], None).unwrap());
    assert!(geometry.batches.is_empty());
}

#[test]
#[ignore = "manual CPU benchmark: run with --ignored --nocapture"]
fn ember_bloom_cached_preview_benchmark() {
    let mut effect = ember();
    for _ in 0..41 {
        effect.step().unwrap();
    }
    let mut geometry = PreviewGeometry::default();
    geometry.prepare(Some(&effect), &PARTICLE_XY_BASIS, &[], None).unwrap();
    let mut samples = Vec::new();
    for _ in 0..60 {
        let start = std::time::Instant::now();
        assert!(!geometry.prepare(Some(std::hint::black_box(&effect)), &PARTICLE_XY_BASIS, &[], None).unwrap());
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    eprintln!("Ember Bloom unchanged preview: median {:.3} ms, p95 {:.3} ms", samples[30], samples[57]);
}
