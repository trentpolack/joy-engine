// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Manual CPU benchmark using the editor's actual Ember Bloom example: building the preview's particle instances. No GPU
//! or frame pacing is included.

use std::hint::black_box;
use std::time::Instant;

use joy_engine::particles::{
    ParticleEffect,
    ParticleEffectOptions,
    ParticleFrame,
    compile_particle_effect,
};

#[test]
#[ignore = "manual CPU benchmark: run with --ignored --nocapture"]
fn ember_bloom_instance_benchmark() {
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
    let mut frame = ParticleFrame::new();
    let mut samples = Vec::new();
    for _ in 0..60 {
        let start = Instant::now();
        frame.clear();
        frame.add_effect(black_box(&effect), None).unwrap();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        black_box(frame.instances());
    }
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "Ember Bloom seed 777, {:.3}s: {} particles, {} instances in {} batches; median {:.4} ms, p95 {:.4} ms",
        effect.time(),
        effect.particles().len(),
        frame.instances().len(),
        frame.batches().len(),
        samples[30],
        samples[57]
    );
}
