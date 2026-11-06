// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Manual CPU benchmark using the editor's actual Ember Bloom example; no GPU or frame pacing is included.

use std::hint::black_box;
use std::time::Instant;

use joy_engine::particles::{
    ParticleCameraBasis,
    ParticleEffect,
    ParticleEffectOptions,
    compile_particle_effect,
    create_particle_effect_batches,
    create_particle_render_metrics,
};
use joy_engine::rendering::OrbitCamera;

#[test]
#[ignore = "manual CPU benchmark: run with --ignored --nocapture"]
fn ember_bloom_geometry_benchmark() {
    let definition = compile_particle_effect(&joyfx_editor::examples::example_json(0)).unwrap();
    let mut effect = ParticleEffect::new(
        &definition,
        ParticleEffectOptions {
            seed: 777,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..41 {
        effect.step().unwrap();
    }
    let camera = ParticleCameraBasis::from(OrbitCamera::default().basis());
    let mut samples = Vec::new();
    let mut triangles = 0;
    for _ in 0..60 {
        let start = Instant::now();
        let batches = create_particle_effect_batches(black_box(&effect), &camera, None, &[]).unwrap();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        triangles = batches.iter().map(|batch| batch.vertex_count() / 3).sum::<usize>();
        black_box(batches);
    }
    samples.sort_by(f64::total_cmp);
    let mut metrics = create_particle_render_metrics(&definition);
    black_box(create_particle_effect_batches(&effect, &camera, Some(&mut metrics), &[]).unwrap());
    eprintln!(
        "Detailed sample: emitters {:?}; sort + batch {:.3} ms",
        metrics.emitters.iter().map(|row| (&row.id, row.geometry_ms)).collect::<Vec<_>>(),
        metrics.sort_ms
    );
    eprintln!(
        "Ember Bloom seed 777, {:.3}s: {} particles, {} triangles; geometry median {:.3} ms, p95 {:.3} ms",
        effect.time(),
        effect.particles().len(),
        triangles,
        samples[30],
        samples[57]
    );
}
