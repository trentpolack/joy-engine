// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Particle simulation, drawing, scripts, and diagnostics checked against a golden recording.
//!
//! `tests/fixtures/particles-parity.json` records the engine's `fire.joyfx`, eight effects from the games, and the
//! engine-authored `lantern.joyfx` (lights, textures, flipbooks, trails, deaths, and vector and color parameters). It
//! holds normalized assets, sampled particles and geometry summaries at several fixed steps, particle lights,
//! frame-cadence results, script evaluation results and diagnostics, and asset and runtime error messages. Each test
//! checks and re-records only its own section (`joy-scene` owns `assetScene`); re-record with `JOY_UPDATE_FIXTURES=1`
//! when particle behavior changes on purpose.
//!
//! Counts, indices, emitter assignments, times, assets, and messages must match exactly. Simulated values must agree to
//! a relative 1e-9: the recorded values came from math libraries whose `sin`, `cos`, `log`, and `pow` differ from the
//! portable FreeBSD implementations in the last bit for a few percent of inputs, and those differences carry forward
//! through integration. Geometry is compared after the renderer's single-precision conversion.

use std::path::Path;

use indexmap::IndexMap;
use joy_core::SeededRandom;
use joy_fx::{
    EffectParticle,
    ParticleAssetError,
    ParticleEffect,
    ParticleEffectOptions,
    ParticleParameterValue,
    ParticleScript,
    ParticleScriptBudget,
    ParticleScriptError,
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
use joy_test_support::{
    GoldenFile,
    Tolerance,
    golden_file,
    json_f32,
    json_f32s,
};
use serde_json::{
    Map,
    Value,
    json,
};

/// Relative tolerance for simulated `f64` values.
const SIMULATION_TOLERANCE: f64 = 1e-9;

/// Relative tolerance for single-precision geometry, its sums, and particle lights.
const GEOMETRY_TOLERANCE: f64 = 2e-6;

/// Relative tolerance for script results.
const SCRIPT_TOLERANCE: f64 = 1e-12;

/// Most particles sampled from one checkpoint, evenly spaced through the particle list.
const SAMPLED_PARTICLES: usize = 10;

/// Values per flattened colored vertex: position and RGBA.
const COLORED_STRIDE: usize = 7;

/// Values per flattened textured vertex: position, RGBA, and UV.
const TEXTURED_STRIDE: usize = 9;

/// Thirty-hertz frames replayed by the cadence check.
const CADENCE_FRAMES: usize = 9;

/// Lights requested from each checkpoint.
const MAX_PARTICLE_LIGHTS: usize = 32;

fn golden() -> GoldenFile {
    golden_file!("particles-parity.json")
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

/// The recorded per-particle columns, in the recording's order.
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

// ---------------------------------------------------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------------------------------------------------

/// Up to [`SAMPLED_PARTICLES`] particles evenly spaced from the first, each as its index (`i`) and columns (`v`).
fn sampled_particles_record(effect: &ParticleEffect) -> Value {
    let particles = effect.particles();
    let stride = particles.len().div_ceil(SAMPLED_PARTICLES).max(1);
    let sampled = particles
        .iter()
        .enumerate()
        .step_by(stride)
        .map(|(index, particle)| json!({"i": index, "v": particle_values(particle).as_slice()}));
    Value::Array(sampled.collect())
}

/// Batch vertices flattened in stream order (stride 7 colored, 9 textured) for sums and leading values.
fn flatten(batch: &ParticleBatch) -> Vec<f32> {
    match batch {
        ParticleBatch::Colored(vertices) => {
            vertices.iter().flat_map(|vertex| vertex.position.iter().chain(&vertex.color).copied()).collect()
        }
        ParticleBatch::Textured { vertices, .. } => vertices
            .iter()
            .flat_map(|vertex| vertex.position.iter().chain(&vertex.color).chain(&vertex.uv).copied())
            .collect(),
    }
}

/// Vertex count, sum, absolute sum, and first vertex of flattened single-precision geometry.
fn geometry_summary(values: &[f32], stride: usize) -> Map<String, Value> {
    let sum: f64 = values.iter().copied().map(f64::from).sum();
    let absolute: f64 = values.iter().map(|value| f64::from(value.abs())).sum();
    let first = &values[..stride.min(values.len())];
    let mut summary = Map::new();
    summary.insert("vertexCount".to_owned(), json!(values.len() / stride));
    summary.insert("sum".to_owned(), json!(sum));
    summary.insert("abs".to_owned(), json!(absolute));
    summary.insert("first".to_owned(), json_f32s(first));
    summary
}

/// One material run: whether it is textured, then its geometry summary.
fn batch_record(batch: &ParticleBatch) -> Value {
    let textured = batch.source().is_some();
    let stride = if textured { TEXTURED_STRIDE } else { COLORED_STRIDE };
    let mut record = Map::new();
    record.insert("textured".to_owned(), json!(textured));
    record.extend(geometry_summary(&flatten(batch), stride));
    Value::Object(record)
}

/// The colored triangle list `append_particle_effect` builds, or `None` for effects it rejects (textured emitters).
fn append_record(effect: &ParticleEffect, camera: &ParticleCameraBasis) -> Option<Value> {
    let mut vertices = Vec::new();
    append_particle_effect(&mut vertices, effect, camera).ok()?;
    let values: Vec<f32> =
        vertices.iter().flat_map(|vertex| vertex.position.iter().chain(&vertex.color).copied()).collect();
    Some(Value::Object(geometry_summary(&values, COLORED_STRIDE)))
}

/// Selected particle lights, each `[x, y, z, r, g, b, range, emitter, particle]` in the renderer's single precision.
fn lights_record(effect: &ParticleEffect) -> Value {
    let selection = collect_particle_lights(effect, MAX_PARTICLE_LIGHTS);
    let lights: Vec<Value> = selection
        .lights
        .iter()
        .map(|selected| {
            let light = &selected.light;
            let mut values: Vec<Value> = light.position.to_array().into_iter().map(json_f32).collect();
            values.extend(light.color.rgb_array().into_iter().map(json_f32));
            values.push(json_f32(light.range));
            values.push(json!(selected.emitter_index));
            values.push(json!(selected.particle_index));
            Value::Array(values)
        })
        .collect();
    json!({"omitted": selection.omitted, "lights": lights})
}

/// Write one checkpoint's state into its recorded `checkpoint`, which keeps the step it was taken at.
fn write_checkpoint(checkpoint: &mut Value, effect: &ParticleEffect, camera: &ParticleCameraBasis) {
    checkpoint["time"] = json!(effect.time());
    checkpoint["count"] = json!(effect.particles().len());
    checkpoint["emitted"] = json!(effect.emitted());
    checkpoint["dropped"] = json!(effect.dropped_particles());
    checkpoint["alive"] = json!(effect.is_alive());
    checkpoint["particles"] = sampled_particles_record(effect);
    let batches = create_particle_effect_batches(effect, camera, None, &[]).unwrap();
    checkpoint["batches"] = Value::Array(batches.iter().map(batch_record).collect());
    match append_record(effect, camera) {
        Some(append) => checkpoint["append"] = append,
        None => {
            checkpoint.as_object_mut().unwrap().shift_remove("append");
        }
    }
    checkpoint["lights"] = lights_record(effect);
}

/// A script's source, written out in full or as repeated parts for long inputs.
fn script_source(case: &Value) -> String {
    match case.get("parts") {
        Some(parts) => parts
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part[0].as_str().unwrap().repeat(part[1].as_u64().unwrap() as usize))
            .collect(),
        None => case["source"].as_str().unwrap().to_owned(),
    }
}

/// A script diagnostic: its message with location, line, and column, marked when compilation rejected the script.
fn script_error_record(error: &ParticleScriptError, compile: bool) -> Value {
    let mut record = json!({"message": error.to_string(), "line": error.line, "column": error.column});
    if compile {
        record["compile"] = json!(true);
    }
    record
}

/// Compile and run one recorded script, writing its diagnostic and its result (or partial result) into `case`.
fn write_script_outcome(case: &mut Value) {
    let source = script_source(case);
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
    let compiled = ParticleScript::compile(&source, &parameters, &containers);

    let outcome = match compiled {
        Err(error) => ScriptOutcome::CompileError(script_error_record(&error, true)),
        Ok(script) => {
            let initial =
                case.get("initial").unwrap_or_else(|| panic!("{label}: compiles but records no initial values"));
            let mut values: IndexMap<String, f64> =
                initial.as_object().unwrap().iter().map(|(name, value)| (name.clone(), number(value))).collect();
            let mut random = SeededRandom::new(case["seed"].as_i64().unwrap());
            let mut budget = ParticleScriptBudget { remaining: case["budget"].as_i64().unwrap() };
            let result = script.run(&mut values, &mut random, &mut budget);
            let values = json!(values);
            match result {
                Ok(()) => ScriptOutcome::Result(values),
                Err(error) => ScriptOutcome::RuntimeError(script_error_record(&error, false), values),
            }
        }
    };

    // Each outcome records only its own fields.
    let fields = case.as_object_mut().unwrap();
    for stale in ["error", "result", "partial"] {
        fields.shift_remove(stale);
    }
    match outcome {
        ScriptOutcome::CompileError(error) => {
            fields.insert("error".to_owned(), error);
        }
        ScriptOutcome::Result(values) => {
            fields.insert("result".to_owned(), values);
        }
        ScriptOutcome::RuntimeError(error, values) => {
            fields.insert("error".to_owned(), error);
            fields.insert("partial".to_owned(), values);
        }
    }
}

/// How one recorded script ended.
enum ScriptOutcome {
    /// Compilation failed with this diagnostic.
    CompileError(Value),
    /// The script ran to completion with these values.
    Result(Value),
    /// The script failed while running with this diagnostic, leaving these values.
    RuntimeError(Value, Value),
}

/// The message an effect document reports when it fails to compile (`null` when it compiles), with the script location
/// for script errors.
fn asset_diagnostic_record(document: &Value) -> Map<String, Value> {
    let mut record = Map::new();
    match compile_particle_effect(document) {
        Ok(_) => {
            record.insert("message".to_owned(), Value::Null);
        }
        Err(error) => {
            record.insert("message".to_owned(), json!(error.to_string()));
            if let ParticleAssetError::Script { source, .. } = &error {
                record.insert("line".to_owned(), json!(source.line));
                record.insert("column".to_owned(), json!(source.column));
            }
        }
    }
    record
}

/// The message a valid effect reports when created with the recorded options and stepped the recorded number of times.
fn runtime_diagnostic(case: &Value) -> String {
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
    result.expect_err("recorded runtime cases fail").to_string()
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden recordings
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn simulations_match_the_recording() {
    let golden = golden();
    let mut effects = golden.recording()["effects"].clone();
    for case in effects.as_array_mut().expect("effects") {
        let file = case["file"].as_str().unwrap().to_owned();
        let definition = compile_particle_effect(&serde_json::from_str(&fixture_text(&file)).unwrap()).unwrap();
        case["asset"] = definition.asset().to_json();

        // Each effect replays its recorded seed, position, parameters, camera basis, and checkpoint steps.
        let position = numbers(&case["position"]);
        let seed = case["seed"].as_i64().unwrap();
        let parameters = parameter_values(&case["parameters"]);
        let options = || ParticleEffectOptions {
            seed,
            position: [position[0], position[1], position[2]],
            parameters: parameters.clone(),
            ..Default::default()
        };
        let camera = basis(&case["basis"]);
        let mut effect = ParticleEffect::new(&definition, options()).unwrap();
        for checkpoint in case["checkpoints"].as_array_mut().unwrap() {
            let step = checkpoint["step"].as_u64().unwrap();
            while effect.ticks() < step {
                effect.step().unwrap();
            }
            write_checkpoint(checkpoint, &effect, &camera);
        }

        // Thirty-hertz frames land on exactly the same fixed-step state.
        let mut cadence = ParticleEffect::new(&definition, options()).unwrap();
        for _ in 0..CADENCE_FRAMES {
            cadence.update(1.0 / 30.0).unwrap();
        }
        case["cadence"] = json!({
            "ticks": cadence.ticks(),
            "count": cadence.particles().len(),
            "particles": sampled_particles_record(&cadence),
        });
    }

    // Assets and fixed-step times are exact; geometry and lights reach the renderer in single precision.
    let tolerance = Tolerance::relative(SIMULATION_TOLERANCE)
        .with_key("asset", 0.0)
        .with_key("time", 0.0)
        .with_key("batches", GEOMETRY_TOLERANCE)
        .with_key("append", GEOMETRY_TOLERANCE)
        .with_key("lights", GEOMETRY_TOLERANCE);
    golden.assert_section_matches("/effects", &effects, &tolerance);
}

#[test]
fn scripts_match_the_recorded_results_and_diagnostics() {
    let golden = golden();
    let mut scripts = golden.recording()["scripts"].clone();
    for case in scripts.as_array_mut().unwrap() {
        write_script_outcome(case);
    }
    golden.assert_section_matches("/scripts", &scripts, &Tolerance::relative(SCRIPT_TOLERANCE));
}

#[test]
fn asset_diagnostics_match_the_recording() {
    let golden = golden();
    let mut cases = golden.recording()["assetErrors"].clone();
    for case in cases.as_array_mut().unwrap() {
        let fields = case.as_object_mut().unwrap();
        for stale in ["message", "name", "line", "column"] {
            fields.shift_remove(stale);
        }
        let record = asset_diagnostic_record(&fields["document"]);
        fields.extend(record);
    }
    golden.assert_section_matches("/assetErrors", &cases, &Tolerance::exact());
}

#[test]
fn runtime_diagnostics_match_the_recording() {
    let golden = golden();
    let mut cases = golden.recording()["runtimeErrors"].clone();
    for case in cases.as_array_mut().unwrap() {
        case["message"] = json!(runtime_diagnostic(case));
    }
    golden.assert_section_matches("/runtimeErrors", &cases, &Tolerance::exact());
}
