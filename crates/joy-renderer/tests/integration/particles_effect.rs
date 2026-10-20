// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle simulation behavior: seeded emission, parameters, children and limits, failures, cadence independence,
//! borrowed random streams, owner edits, and profiling.

use joy_core::LinearColor;
use std::cell::Cell;
use std::rc::Rc;

use joy_core::RandomSource;
use joy_fx::{
    CompiledParticleEffect,
    ParticleEffect,
    ParticleEffectError,
    ParticleEffectOptions,
    ParticleParameterValue,
    compile_particle_effect,
};
use joy_renderer::particles::collect_particle_lights;
use serde_json::{
    Value,
    json,
};

/// A one-emitter test document (a two-particle burst driven by a `speed` parameter) with emitter overrides merged in.
fn document(overrides: Value) -> Value {
    let mut emitter = json!({"id": "main", "enabled": true, "root": true, "duration": 0, "rate": 0,
        "bursts": [{"time": 0, "count": 2}], "lifetime": 1, "renderer": "soft",
        "spawn": "vx = speed; size = rand(2, 4);", "update": "alpha = 1 - t;"});
    if let (Value::Object(emitter), Value::Object(overrides)) = (&mut emitter, overrides) {
        emitter.extend(overrides);
    }
    json!({"version": 1, "name": "Test", "maxParticles": 64, "parameters": {"speed": {"value": 60, "min": 0, "max": 120}},
        "emitters": [emitter]})
}

fn compile(document: &Value) -> CompiledParticleEffect {
    compile_particle_effect(document).unwrap_or_else(|error| panic!("{error}"))
}

fn effect(document: &Value) -> ParticleEffect {
    ParticleEffect::new(&compile(document), ParticleEffectOptions::default()).unwrap()
}

#[test]
fn seeded_effects_use_seconds_and_independent_parameter_overrides() {
    let definition = compile(&document(json!({})));
    let mut first = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let second = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    assert_eq!(first.particles(), second.particles());
    assert_eq!(first.particles().len(), 2);
    first.step().unwrap();
    assert_eq!(first.particles()[0].x, 1.0);
    assert_eq!(second.particles()[0].x, 0.0);

    let mut fast = ParticleEffect::new(
        &definition,
        ParticleEffectOptions {
            parameters: vec![("speed".to_owned(), ParticleParameterValue::Scalar(120.0))],
            position: [12.0, 34.0, 5.0],
            ..Default::default()
        },
    )
    .unwrap();
    fast.step().unwrap();
    assert_eq!(fast.particles()[0].x, 14.0);
    assert_eq!(fast.particles()[0].z, 5.0);
    first.reset().unwrap();
    assert_eq!(first.particles(), second.particles());
}

#[test]
fn scripts_keep_locals_conditions_and_persistent_channels_across_seek() {
    let mut runtime = effect(&document(json!({
        "spawn": "let angle = pi / 2; vx = cos(angle); vy = sin(angle); u0 = 2;",
        "update": "u0 += dt; if(t < 0.5) { r = 8; } else { r = 1; } size = mix(2, 8, t);"
    })));
    runtime.step().unwrap();
    assert_eq!(runtime.particles()[0].r, 8.0);
    assert!(runtime.particles()[0].u0 > 2.0);
    runtime.seek(0.6).unwrap();
    assert_eq!(runtime.particles()[0].r, 1.0);
    assert!(runtime.particles()[0].size > 5.0);
}

#[test]
fn trails_and_deaths_follow_the_moving_parent_and_finish() {
    let mut source = document(json!({"bursts": [{"time": 0, "count": 1}], "lifetime": 0.1,
        "trail": {"emitter": "smoke", "interval": 1.0 / 30.0, "count": 1}, "death": {"emitter": "smoke", "count": 3}}));
    let smoke = json!({"id": "smoke", "enabled": true, "root": false, "duration": 0, "rate": 0,
        "bursts": [{"time": 0, "count": 2}], "lifetime": 0.2, "renderer": "soft", "spawn": "vx = parentVx * 0.1;", "update": ""});
    source["emitters"].as_array_mut().unwrap().push(smoke);
    let mut runtime = effect(&source);
    runtime.seek(0.1).unwrap();
    let children: Vec<_> = runtime.particles().iter().filter(|particle| particle.emitter_index == 1).collect();
    assert!(children.len() >= 5);
    assert!(children.iter().all(|particle| particle.x > 0.0 && particle.generation == 1));
    runtime.seek(1.0).unwrap();
    assert!(!runtime.is_alive());
}

#[test]
fn capacity_and_generation_limits_bound_recursive_children() {
    let mut source = document(json!({"lifetime": 1.0 / 60.0, "death": {"emitter": "main", "count": 4096}}));
    source["maxParticles"] = json!(8);
    let mut runtime = effect(&source);
    for _ in 0..20 {
        runtime.step().unwrap();
        assert!(runtime.particles().len() <= 8);
    }
    assert!(runtime.dropped_particles() > 0);
    assert!(!runtime.is_alive());
    runtime.destroy();
    runtime.destroy();
    assert!(runtime.particles().is_empty());
    assert_eq!(runtime.step(), Err(ParticleEffectError::Destroyed));
}

#[test]
fn stopping_drains_live_particles_and_failures_stop_the_effect() {
    let mut runtime = effect(&document(json!({"duration": 2, "rate": 60, "bursts": [], "lifetime": 0.1})));
    runtime.step().unwrap();
    runtime.stop();
    for _ in 0..30 {
        runtime.step().unwrap();
    }
    assert!(!runtime.is_alive());

    // A failing update stops emission, keeps the error, and returns it until reset.
    let mut failing = effect(&document(json!({"update": "if (age > 0.05) { x = 1 / 0; }"})));
    let error = (0..10).find_map(|_| failing.step().err()).expect("the update fails");
    assert!(error.to_string().starts_with("Emitter 'main' update: Numeric result must be finite"), "{error}");
    assert!(failing.is_stopped());
    assert_eq!(failing.step(), Err(error));
    failing.reset().unwrap();
    assert!(failing.error().is_none());
}

#[test]
fn parameter_names_never_alias_private_bookkeeping() {
    let mut source = document(json!({"spawn": "size = generation + emitterIndex + nextTrail;"}));
    source["parameters"] = json!({"generation": {"value": 2, "min": 0, "max": 10},
        "emitterIndex": {"value": 3, "min": 0, "max": 10}, "nextTrail": {"value": 4, "min": 0, "max": 10}});
    assert_eq!(effect(&source).particles()[0].size, 9.0);
}

#[test]
fn frame_cadence_and_seek_preserve_randomized_trajectories() {
    let definition = compile(&document(json!({"duration": 2, "rate": 37, "bursts": [{"time": 0.1, "count": 3}],
        "spawn": "vx = rand(20, 80); vy = rand(-50, 50); u0 = rand();", "update": "ax = noise(age + u0);"})));
    let create = || ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let (mut fine, mut coarse, mut sought) = (create(), create(), create());
    for _ in 0..48 {
        fine.update(1.0 / 120.0).unwrap();
    }
    for _ in 0..12 {
        coarse.update(1.0 / 30.0).unwrap();
    }
    sought.seek(0.4).unwrap();
    assert!(fine.particles().len() > 10);
    assert_eq!(fine.particles(), coarse.particles());
    assert_eq!(fine.particles(), sought.particles());
    assert_eq!(fine.update(0.3), Err(ParticleEffectError::InvalidDelta));
}

/// A borrowed game stream that counts draws.
struct CountingRandom(Rc<Cell<u32>>);

impl RandomSource for CountingRandom {
    fn next_unit(&mut self) -> f64 {
        self.0.set(self.0.get() + 1);
        0.25
    }
}

#[test]
fn borrowed_random_streams_continue_across_reset() {
    let calls = Rc::new(Cell::new(0));
    let options = ParticleEffectOptions {
        random: Some(Box::new(CountingRandom(calls.clone()))),
        ..Default::default()
    };
    let mut runtime = ParticleEffect::new(&compile(&document(json!({}))), options).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(runtime.particles()[0].size, 2.5);
    runtime.reset().unwrap();
    assert_eq!(calls.get(), 4, "reset continues a borrowed stream");
}

#[test]
fn owners_cull_oldest_particles_and_rescale_positions_atomically() {
    let definition = compile(&document(json!({})));
    let options = ParticleEffectOptions {
        position: [10.0, 20.0, 3.0],
        ..Default::default()
    };
    let mut runtime = ParticleEffect::new(&definition, options).unwrap();
    runtime.step().unwrap();
    let newest = runtime.particles()[1].clone();
    runtime.remove_oldest_particles(1).unwrap();
    assert_eq!(runtime.particles(), [newest]);
    runtime.scale_positions([2.0, 0.5, 1.0]).unwrap();
    assert_eq!(runtime.particles()[0].x, 22.0);
    assert_eq!(runtime.particles()[0].y, 10.0);
    assert_eq!(runtime.particles()[0].vx, 60.0);
    assert_eq!(runtime.scale_positions([f64::INFINITY, 1.0, 1.0]), Err(ParticleEffectError::InvalidScale));
    assert_eq!(runtime.particles()[0].x, 22.0, "invalid scaling is atomic");
    assert_eq!(runtime.position(), [20.0, 10.0, 3.0]);
}

#[test]
fn vector_and_color_parameters_are_typed_read_only_inputs() {
    let mut source =
        document(json!({"spawn": "velocity = direction; color = tint; size = tint.a + direction.z;", "update": ""}));
    source["parameters"] = json!({
        "direction": {"type": "vector3", "value": [1, 2, 3], "min": [-10, -10, -10], "max": [10, 10, 10]},
        "tint": {"type": "color", "value": [0.2, 0.4, 0.6, 1], "min": [0, 0, 0, 0], "max": [1, 1, 1, 1]}
    });
    let definition = compile(&source);
    let tint = ParticleParameterValue::Color([0.8, 0.7, 0.6, 0.5]);
    let options = || ParticleEffectOptions {
        parameters: vec![("tint".to_owned(), tint)],
        ..Default::default()
    };
    let mut runtime = ParticleEffect::new(&definition, options()).unwrap();
    let particle = &runtime.particles()[0];
    assert_eq!([particle.r, particle.alpha, particle.size, particle.vy], [0.8, 0.5, 3.5, 2.0]);
    assert_eq!(runtime.parameter("tint.r"), Some(0.8));
    assert_eq!(runtime.parameter("directionZ"), Some(3.0));
    runtime.reset().unwrap();
    assert_eq!(runtime.particles()[0].r, 0.8);

    let out_of_range = ParticleParameterValue::Color([1.0, 0.0, 0.0, 2.0]);
    let wrong_shape = ParticleParameterValue::Scalar(1.0);
    for value in [out_of_range, wrong_shape] {
        let options = ParticleEffectOptions {
            parameters: vec![("tint".to_owned(), value)],
            ..Default::default()
        };
        let error = ParticleEffect::new(&definition, options).unwrap_err();
        assert_eq!(error.to_string(), "Invalid value for effect parameter 'tint'.");
    }
}

#[test]
fn version_one_names_shadow_built_in_containers() {
    for name in ["position", "velocity", "acceleration", "color", "origin", "parentVelocity", "vector", "rgba"] {
        let mut parameter = document(json!({"spawn": format!("vx = {name};"), "update": ""}));
        parameter["parameters"][name] = json!({"value": 3, "min": 0, "max": 4});
        assert_eq!(effect(&parameter).particles()[0].vx, 3.0, "parameter {name}");
        let local = document(json!({"spawn": format!("let {name} = 3; vx = {name};"), "update": ""}));
        assert_eq!(effect(&local).particles()[0].vx, 3.0, "local {name}");
    }
}

#[test]
fn profiling_preserves_trajectories_and_counts_work() {
    let source = document(json!({"duration": 2, "rate": 37, "renderer": "square",
        "spawn": "vx = rand(1, 2); r = 2; g = 1; b = 0.5;"}));
    let definition = compile(&source);
    let clock = Rc::new(Cell::new(0.0));
    let ticking = clock.clone();
    let mut plain = ParticleEffect::new(
        &definition,
        ParticleEffectOptions {
            seed: 31,
            profile_clock: Some(Box::new(|| panic!("the clock is read only while profiling"))),
            ..Default::default()
        },
    )
    .unwrap();
    let mut profiled = ParticleEffect::new(
        &definition,
        ParticleEffectOptions {
            seed: 31,
            profiling: true,
            profile_clock: Some(Box::new(move || {
                ticking.set(ticking.get() + 1.0);
                ticking.get()
            })),
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..24 {
        plain.step().unwrap();
        profiled.step().unwrap();
    }
    assert_eq!(plain.particles(), profiled.particles());
    let profile = profiled.profile();
    assert_eq!(profile.steps, 24);
    assert!(profile.simulation_ms > 0.0 && profile.emitters[0].spawn_ms > 0.0 && profile.emitters[0].update_ms > 0.0);
    assert_eq!(profile.emitters[0].live, profiled.particles().len() as u64);
    assert!(profile.emitters[0].operations > 0);
    profiled.reset().unwrap();
    assert_eq!(profiled.profile().steps, 0);
    assert_eq!(profiled.profile().emitters[0].spawned, 2);
    assert!(clock.get() > 0.0);

    // Toggling profiling at run time never restarts the simulation; enabling starts fresh totals with live counts.
    plain.set_profiling(true);
    assert_eq!(plain.profile().steps, 0);
    assert_eq!(plain.profile().emitters[0].live, plain.particles().len() as u64);
    let particles = plain.particles().to_vec();
    plain.set_profiling(false);
    assert_eq!(plain.particles(), particles.as_slice());
    assert!(!plain.profile().enabled);

    // Root and child drops are both counted.
    let mut limited = document(json!({"lifetime": 1.0 / 60.0, "spawn": "x=1;", "update": "x+=1;",
        "death": {"emitter": "child", "count": 3}}));
    limited["maxParticles"] = json!(2);
    let child = json!({"id": "child", "enabled": true, "root": false, "duration": 0, "rate": 0, "bursts": [],
        "lifetime": 1, "renderer": "soft", "spawn": "x=1;", "update": "x+=1;"});
    limited["emitters"].as_array_mut().unwrap().push(child);
    let options = ParticleEffectOptions {
        profiling: true,
        ..Default::default()
    };
    let mut runtime = ParticleEffect::new(&compile(&limited), options).unwrap();
    assert_eq!(runtime.profile().emitters[0].operations, 2);
    runtime.step().unwrap();
    assert_eq!(runtime.profile().emitters[0].operations, 4);
    assert_eq!(runtime.profile().emitters[1].spawned, 2);
    assert_eq!(runtime.profile().emitters[1].dropped, 4);
    assert_eq!(runtime.dropped_particles(), 4);
}

#[test]
fn lights_rank_by_luminance_with_stable_ties_and_skip_invalid_particles() {
    let lit = |spawn: &str, count: u32| {
        let source = document(json!({"bursts": [{"time": 0, "count": count}], "renderer": "square",
            "light": {"intensity": 1, "range": 20}, "spawn": spawn}));
        effect(&source)
    };
    let runtime = lit("r = index; g = 0; b = 0;", 40);
    assert_eq!(collect_particle_lights(&runtime, 100).lights.len(), 39, "the black particle emits nothing");
    let selected = collect_particle_lights(&runtime, 32);
    assert_eq!(selected.lights.len(), 32);
    assert_eq!(selected.omitted, 7);
    let order: Vec<u64> = selected.lights.iter().map(|light| light.particle_index).collect();
    assert_eq!(order, (8..40).rev().collect::<Vec<_>>());
    assert_eq!(collect_particle_lights(&runtime, 0).omitted, 39);

    let ties = lit("r = 2; g = 1; b = 0.5;", 2);
    let order: Vec<u64> = collect_particle_lights(&ties, 32).lights.iter().map(|light| light.particle_index).collect();
    assert_eq!(order, [0, 1]);
    for spawn in ["lightIntensity = 0;", "lightRange = 0;", "alpha = 0;", "lifetime = 0;"] {
        assert!(collect_particle_lights(&lit(spawn, 1), 32).lights.is_empty(), "{spawn}");
    }
    let unlit = effect(&document(json!({"spawn": "lightIntensity = 4; lightRange = 20;"})));
    assert!(collect_particle_lights(&unlit, 32).lights.is_empty(), "emitters without a light record never emit");
    let point = collect_particle_lights(&ties, 32).point_lights();
    assert_eq!(point[0].color, LinearColor::rgb(2.0, 1.0, 0.5));
}
