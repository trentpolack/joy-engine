// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle parity against the TypeScript engine.
//!
//! `tests/fixtures/particles-parity.json` was recorded from the TypeScript `src/particles/` runtime (Node 22) for the
//! engine's `fire.joyfx`, eight effects from the games, and the engine-authored `lantern.joyfx` (lights, textures,
//! flipbooks, trails, deaths, and vector and color parameters). It holds normalized assets, sampled particles and
//! geometry summaries at several fixed steps, particle lights, frame-cadence results, script evaluation results and
//! diagnostics, and asset and runtime error messages.
//!
//! Counts, indices, emitter assignments, and messages must match exactly. Simulated values must agree to a relative
//! 1e-9: V8's `sin`, `cos`, `log`, and `pow` differ from the portable FreeBSD implementations in the last bit for a few
//! percent of inputs, and those differences carry forward through integration. Geometry is compared after the
//! renderer's single-precision conversion.

use std::path::Path;

use indexmap::IndexMap;
use joy_core::SeededRandom;
use joy_fx::{
    EffectParticle,
    ParticleEffect,
    ParticleEffectOptions,
    ParticleParameterValue,
    ParticleScript,
    ParticleScriptBudget,
    ParticleValueKind,
    compile_particle_effect,
    particle_seed,
};
use joy_renderer::particles::{
    ParticleBatch,
    ParticleCameraBasis,
    append_particle_effect,
    collect_particle_lights,
    create_particle_effect_batches,
};
use serde_json::Value;

/// Relative tolerance for simulated `f64` values.
const SIMULATION_TOLERANCE: f64 = 1e-9;

/// Relative tolerance for single-precision geometry and its sums.
const GEOMETRY_TOLERANCE: f64 = 2e-6;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/particles-parity.json")).expect("parity fixture is JSON")
}

fn fixture_text(file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or_else(|| panic!("expected a number, found {value}"))
}

fn numbers(value: &Value) -> Vec<f64> {
    value.as_array().expect("array").iter().map(number).collect()
}

fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
    actual == expected || (actual - expected).abs() <= tolerance * (1.0 + expected.abs())
}

/// The recorded per-particle columns, in the recorder's order.
fn particle_values(particle: &EffectParticle) -> [f64; 34] {
    let fields = particle.fields();
    let mut values = [0.0; 34];
    values[..20].copy_from_slice(&fields[..20]);
    values[20] = particle.age;
    values[21..25].copy_from_slice(&fields[20..24]);
    values[25] = particle.emitter_index as f64;
    values[26] = f64::from(particle.generation);
    values[27] = particle.index as f64;
    values[28..34].copy_from_slice(&[
        particle.origin_x,
        particle.origin_y,
        particle.origin_z,
        particle.parent_vx,
        particle.parent_vy,
        particle.parent_vz,
    ]);
    values
}

/// Compare JSON documents, treating numbers by value (`1` equals `1.0`).
fn assert_json_equivalent(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(left), Value::Number(right)) => {
            assert_eq!(left.as_f64(), right.as_f64(), "{path}");
        }
        (Value::Array(left), Value::Array(right)) => {
            assert_eq!(left.len(), right.len(), "{path} length");
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                assert_json_equivalent(left, right, &format!("{path}[{index}]"));
            }
        }
        (Value::Object(left), Value::Object(right)) => {
            let left_keys: Vec<&String> = left.keys().collect();
            let right_keys: Vec<&String> = right.keys().collect();
            assert_eq!(left_keys, right_keys, "{path} keys");
            for (key, value) in left {
                assert_json_equivalent(value, &right[key], &format!("{path}.{key}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

fn parameter_values(record: &Value) -> Vec<(String, ParticleParameterValue)> {
    record
        .as_object()
        .expect("parameters object")
        .iter()
        .map(|(name, value)| (name.clone(), ParticleParameterValue::from_json(value).expect("parameter value")))
        .collect()
}

fn basis(record: &Value) -> ParticleCameraBasis {
    let axis = |name: &str| {
        let values = numbers(&record[name]);
        [values[0], values[1], values[2]]
    };
    ParticleCameraBasis { right: axis("right"), up: axis("up"), backward: axis("backward") }
}

fn assert_particles(effect: &ParticleEffect, sampled: &Value, label: &str) {
    for entry in sampled.as_array().expect("sampled particles") {
        let index = entry["i"].as_u64().expect("index") as usize;
        let expected = numbers(&entry["v"]);
        let actual = particle_values(&effect.particles()[index]);
        for (column, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
            assert!(
                close(*actual, *expected, SIMULATION_TOLERANCE),
                "{label} particle {index} column {column}: {actual} != {expected}"
            );
        }
    }
}

/// Flatten batches like the TypeScript stream (stride 7 colored, 9 textured) for sums and leading values.
fn flatten(batch: &ParticleBatch) -> Vec<f64> {
    match batch {
        ParticleBatch::Colored(vertices) => vertices
            .iter()
            .flat_map(|vertex| {
                vertex.position.iter().chain(&vertex.color).map(|value| f64::from(*value)).collect::<Vec<_>>()
            })
            .collect(),
        ParticleBatch::Textured { vertices, .. } => vertices
            .iter()
            .flat_map(|vertex| {
                vertex
                    .position
                    .iter()
                    .chain(&vertex.color)
                    .chain(&vertex.uv)
                    .map(|value| f64::from(*value))
                    .collect::<Vec<_>>()
            })
            .collect(),
    }
}

fn assert_summary(values: &[f64], stride: usize, expected: &Value, label: &str) {
    assert_eq!(values.len() / stride, expected["vertexCount"].as_u64().unwrap() as usize, "{label} vertex count");
    let sum: f64 = values.iter().sum();
    let absolute: f64 = values.iter().map(|value| value.abs()).sum();
    let expected_absolute = number(&expected["abs"]);
    // Sums cancel, so compare them against the magnitude of everything summed.
    let scale = 1.0 + expected_absolute;
    assert!(
        (sum - number(&expected["sum"])).abs() <= GEOMETRY_TOLERANCE * scale,
        "{label} sum {sum} vs {}",
        expected["sum"]
    );
    assert!((absolute - expected_absolute).abs() <= GEOMETRY_TOLERANCE * scale, "{label} abs {absolute}");
    for (actual, expected) in values.iter().zip(numbers(&expected["first"])) {
        assert!(
            (actual - expected).abs() <= GEOMETRY_TOLERANCE * (1.0 + expected.abs()),
            "{label} first {actual} vs {expected}"
        );
    }
}

#[test]
fn simulations_match_typescript_recordings() {
    let fixture = fixture();
    for case in fixture["effects"].as_array().expect("effects") {
        let file = case["file"].as_str().unwrap();
        let text = fixture_text(file);
        let definition = compile_particle_effect(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_json_equivalent(&definition.asset().to_json(), &case["asset"], file);

        let position = numbers(&case["position"]);
        let options = || ParticleEffectOptions {
            seed: case["seed"].as_i64().unwrap(),
            position: [position[0], position[1], position[2]],
            parameters: parameter_values(&case["parameters"]),
            ..Default::default()
        };
        let camera = basis(&case["basis"]);
        let mut effect = ParticleEffect::new(&definition, options()).unwrap();
        for checkpoint in case["checkpoints"].as_array().unwrap() {
            let step = checkpoint["step"].as_u64().unwrap();
            while effect.ticks() < step {
                effect.step().unwrap();
            }
            let label = format!("{file} step {step}");
            assert_eq!(effect.time(), number(&checkpoint["time"]), "{label} time");
            assert_eq!(effect.particles().len() as u64, checkpoint["count"].as_u64().unwrap(), "{label} count");
            assert_eq!(effect.emitted(), checkpoint["emitted"].as_u64().unwrap(), "{label} emitted");
            assert_eq!(effect.dropped_particles(), checkpoint["dropped"].as_u64().unwrap(), "{label} dropped");
            assert_eq!(effect.is_alive(), checkpoint["alive"].as_bool().unwrap(), "{label} alive");
            assert_particles(&effect, &checkpoint["particles"], &label);

            let batches = create_particle_effect_batches(&effect, &camera, None, &[]).unwrap();
            let expected_batches = checkpoint["batches"].as_array().unwrap();
            assert_eq!(batches.len(), expected_batches.len(), "{label} batch count");
            for (index, (batch, expected)) in batches.iter().zip(expected_batches).enumerate() {
                let textured = batch.source().is_some();
                assert_eq!(textured, expected["textured"].as_bool().unwrap(), "{label} batch {index} material");
                assert_summary(
                    &flatten(batch),
                    if textured { 9 } else { 7 },
                    expected,
                    &format!("{label} batch {index}"),
                );
            }

            if let Some(expected) = checkpoint.get("append") {
                let mut vertices = Vec::new();
                append_particle_effect(&mut vertices, &effect, &camera).unwrap();
                let values: Vec<f64> = vertices
                    .iter()
                    .flat_map(|vertex| {
                        vertex.position.iter().chain(&vertex.color).map(|value| f64::from(*value)).collect::<Vec<_>>()
                    })
                    .collect();
                assert_summary(&values, 7, expected, &format!("{label} append"));
            }

            let lights = collect_particle_lights(&effect, 32);
            let expected_lights = checkpoint["lights"]["lights"].as_array().unwrap();
            assert_eq!(
                lights.omitted as u64,
                checkpoint["lights"]["omitted"].as_u64().unwrap(),
                "{label} omitted lights"
            );
            assert_eq!(lights.lights.len(), expected_lights.len(), "{label} light count");
            for (selected, expected) in lights.lights.iter().zip(expected_lights) {
                let expected = numbers(expected);
                let light = &selected.light;
                let mut actual = light.position.to_array().to_vec();
                actual.extend(light.color.rgb_array());
                actual.push(light.range);
                // Lights reach the renderer in single precision, like geometry.
                for (actual, expected) in actual.iter().zip(&expected) {
                    let actual = f64::from(*actual);
                    assert!(close(actual, *expected, GEOMETRY_TOLERANCE), "{label} light {actual} != {expected}");
                }
                let light = selected;
                assert_eq!(light.emitter_index as f64, expected[7], "{label} light emitter");
                assert_eq!(light.particle_index as f64, expected[8], "{label} light particle");
            }
        }

        // Thirty-hertz frames land on exactly the same fixed-step state.
        let mut cadence = ParticleEffect::new(&definition, options()).unwrap();
        for _ in 0..9 {
            cadence.update(1.0 / 30.0).unwrap();
        }
        assert_eq!(cadence.ticks(), case["cadence"]["ticks"].as_u64().unwrap(), "{file} cadence ticks");
        assert_eq!(
            cadence.particles().len() as u64,
            case["cadence"]["count"].as_u64().unwrap(),
            "{file} cadence count"
        );
        assert_particles(&cadence, &case["cadence"]["particles"], &format!("{file} cadence"));
    }
}

#[test]
fn scripts_match_typescript_results_and_diagnostics() {
    let fixture = fixture();
    for case in fixture["scripts"].as_array().unwrap() {
        let source = match case.get("parts") {
            Some(parts) => parts
                .as_array()
                .unwrap()
                .iter()
                .map(|part| part[0].as_str().unwrap().repeat(part[1].as_u64().unwrap() as usize))
                .collect::<String>(),
            None => case["source"].as_str().unwrap().to_owned(),
        };
        let label: String = source.chars().take(48).collect();
        let parameters: Vec<&str> =
            case["parameters"].as_array().unwrap().iter().map(|name| name.as_str().unwrap()).collect();
        let containers: Vec<(&str, ParticleValueKind)> = case["containers"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, kind)| {
                let kind = if kind == "vector" { ParticleValueKind::Vector } else { ParticleValueKind::Color };
                (name.as_str(), kind)
            })
            .collect();

        let error = case.get("error");
        let compiled = ParticleScript::compile(&source, &parameters, &containers);
        if let Some(error) = error.filter(|error| error.get("compile").is_some()) {
            let actual = compiled.expect_err(&label);
            assert_eq!(actual.to_string(), error["message"].as_str().unwrap(), "{label}");
            assert_eq!(f64::from(actual.line), number(&error["line"]), "{label} line");
            assert_eq!(f64::from(actual.column), number(&error["column"]), "{label} column");
            continue;
        }
        let script = compiled.unwrap_or_else(|error| panic!("{label}: {error}"));
        let mut values: IndexMap<String, f64> =
            case["initial"].as_object().unwrap().iter().map(|(name, value)| (name.clone(), number(value))).collect();
        let mut random = SeededRandom::new(case["seed"].as_i64().unwrap());
        let mut budget = ParticleScriptBudget { remaining: case["budget"].as_i64().unwrap() };
        let result = script.run(&mut values, &mut random, &mut budget);

        let expected_values = match error {
            Some(error) => {
                let actual = result.expect_err(&label);
                assert_eq!(actual.to_string(), error["message"].as_str().unwrap(), "{label}");
                &case["partial"]
            }
            None => {
                result.unwrap_or_else(|error| panic!("{label}: {error}"));
                &case["result"]
            }
        };
        let expected_values = expected_values.as_object().unwrap();
        assert_eq!(values.len(), expected_values.len(), "{label} value names");
        for (name, expected) in expected_values {
            let actual = values.get(name).copied().unwrap_or_else(|| panic!("{label}: missing {name}"));
            assert!(close(actual, number(expected), 1e-12), "{label}: {name} = {actual}, expected {expected}");
        }
    }
}

#[test]
fn asset_diagnostics_match_typescript_messages() {
    let fixture = fixture();
    for case in fixture["assetErrors"].as_array().unwrap() {
        let result = compile_particle_effect(&case["document"]);
        match case["message"].as_str() {
            None => assert!(result.is_ok(), "{}: {:?}", case["document"], result.err()),
            Some(message) => assert_eq!(result.unwrap_err().to_string(), message, "{}", case["document"]),
        }
    }
}

#[test]
fn runtime_diagnostics_match_typescript_messages() {
    let fixture = fixture();
    for case in fixture["runtimeErrors"].as_array().unwrap() {
        let definition = compile_particle_effect(&case["document"]).unwrap();
        let options = &case["options"];
        let result = (|| {
            let seed = match options.get("seed") {
                Some(seed) => i64::from(particle_seed(number(seed))?),
                None => 0,
            };
            let position = match options.get("position") {
                Some(position) => [number(&position["x"]), number(&position["y"]), 0.0],
                None => [0.0; 3],
            };
            let parameters = options.get("parameters").map(parameter_values).unwrap_or_default();
            let mut effect = ParticleEffect::new(
                &definition,
                ParticleEffectOptions { seed, position, parameters, ..Default::default() },
            )?;
            for _ in 0..case["steps"].as_u64().unwrap() {
                effect.step()?;
            }
            Ok::<(), joy_fx::ParticleEffectError>(())
        })();
        assert_eq!(result.unwrap_err().to_string(), case["message"].as_str().unwrap(), "{}", case["document"]);
    }
}
