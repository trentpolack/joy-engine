// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Bundled examples and emitter presets: examples compile with the current scripting syntax, and presets compile
//! without shared parameters or child references and are detached copies.

use joy_engine::particles::compile_particle_effect;
use particle_lab::constants::EmitterPreset;
use particle_lab::examples::{
    EMITTER_PRESETS,
    EXAMPLES,
    copy_example,
    create_emitter_template,
    example_json,
};

#[test]
fn bundled_examples_compile_with_current_aggregate_scripting_syntax() {
    for (index, example) in EXAMPLES.iter().enumerate() {
        let compiled =
            compile_particle_effect(&example_json(index)).unwrap_or_else(|error| panic!("{}: {error}", example.label));
        assert_eq!(
            compiled.emitters().len(),
            example_json(index)["emitters"].as_array().unwrap().len(),
            "{}",
            example.label
        );
        assert_eq!(copy_example(index).name, compiled.asset().name);
    }
    // The default template compiles alone in a minimal effect.
    let mut starter = copy_example(0);
    starter.name = "Starter".to_owned();
    starter.max_particles = 128;
    starter.parameters.clear();
    starter.emitters = vec![create_emitter_template("starter", EmitterPreset::Spray)];
    assert!(compile_particle_effect(&starter.to_json()).is_ok());
}

#[test]
fn emitter_presets_compile_without_shared_parameters_or_child_references_and_are_detached() {
    for info in EMITTER_PRESETS {
        let mut emitter = create_emitter_template("new_emitter", info.preset);
        let mut asset = copy_example(0);
        asset.parameters.clear();
        asset.max_particles = 512;
        asset.emitters = vec![emitter.clone()];
        assert!(compile_particle_effect(&asset.to_json()).is_ok(), "{}", info.label);
        assert_eq!(emitter.id, "new_emitter");
        assert!(emitter.trail.is_none() && emitter.death.is_none());
        emitter.bursts.push(joy_engine::particles::ParticleBurst {
            time: 1.0,
            count: 1,
        });
        assert_ne!(create_emitter_template("new_emitter", info.preset).bursts, emitter.bursts);
    }
}
