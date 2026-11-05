// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The sandbox: runaway scripts stop at their budget, scripts cannot reach files, math matches the engine's portable
//! Rust helpers, seeded draws interleave with host draws in one stream, and lent state never outlives its call.

use joy_core::RandomSource;
use joy_core::SeededRandom;
use joy_core::math::DVec3;
use joy_core::math::deterministic;
use joy_game_script::Script;
use joy_game_script::ScriptEngine;
use joy_game_script::ScriptError;
use joy_game_script::ScriptHook;
use joy_game_script::ScriptLimits;
use joy_game_script::ScriptLoan;
use joy_game_script::ScriptRandom;
use joy_game_script::ScriptState;

const PATH: &str = "sandbox.rhai";

#[test]
fn a_runaway_loop_stops_at_the_operation_budget_and_the_next_call_runs() {
    let engine = ScriptEngine::new(ScriptLimits {
        operations_per_call: 10_000,
        ..ScriptLimits::default()
    });
    let hooks = [ScriptHook::new("spin", 1)];
    let script = Script::compile(&engine, PATH, "fn spin(forever) { while forever {} 1 }", &hooks).unwrap();
    let mut state = ScriptState::new();

    let error = script.call(&engine, "spin", &mut state, (true,)).unwrap_err();
    assert!(
        matches!(error, ScriptError::Runtime { ref message, .. } if message.contains("operation budget")),
        "{error}"
    );
    assert_eq!(script.call_as::<i64>(&engine, "spin", &mut state, (false,)).unwrap(), 1);
}

#[test]
fn scripts_cannot_import_files_or_use_the_platform_power_operator() {
    let engine = ScriptEngine::default();
    let import = Script::compile(&engine, PATH, "import \"secrets\" as secrets;", &[]).unwrap_err();
    assert!(matches!(import, ScriptError::Compile { .. }), "{import}");
    let power = Script::compile(&engine, PATH, "fn grow(x) { x ** 2.0 }", &[]).unwrap_err();
    assert!(matches!(power, ScriptError::Compile { .. }), "{power}");
}

#[test]
fn script_math_and_seeded_draws_match_the_rust_helpers_exactly() {
    let engine = ScriptEngine::default();
    let hooks = [ScriptHook::new("sample", 2)];
    let source = r#"
        fn sample(heading, random) {
            let direction = vec3(sin(heading), 0.0, cos(heading)) * 20.0;
            let offset = direction - vec3(1.5, 0.0, -2.0);
            [hypot(offset.x, offset.z), offset.length(), random.next_unit(), random.range(2.0, 4.0)]
        }
    "#;
    let script = Script::compile(&engine, PATH, source, &hooks).unwrap();
    let random = ScriptRandom::new();
    let mut stream = SeededRandom::new(91);
    let mut expected_stream = SeededRandom::new(91);

    // The host and the script draw from one stream, in call order.
    let host_draw = stream.next_unit();
    let mut state = ScriptState::new();
    let values: Vec<f64> = random.lend(&mut stream, || {
        let values =
            script.call_as::<joy_game_script::rhai::Array>(&engine, "sample", &mut state, (0.7, random.clone()));
        values.unwrap().into_iter().map(|value| value.cast::<f64>()).collect()
    });
    let after = stream.next_unit();

    let direction = DVec3::new(deterministic::sin(0.7), 0.0, deterministic::cos(0.7)) * 20.0;
    let offset = direction - DVec3::new(1.5, 0.0, -2.0);
    assert_eq!(host_draw, expected_stream.next_unit());
    assert_eq!(values[0], deterministic::hypot(offset.x, offset.z));
    assert_eq!(values[1], offset.length());
    assert_eq!(values[2], expected_stream.next_unit());
    assert_eq!(values[3], 2.0 + (expected_stream.next_unit() * 2.0));
    assert_eq!(after, expected_stream.next_unit(), "the lent stream returns to the host advanced");
}

#[test]
fn float_comparisons_are_exact_like_rust() {
    // Fifteen 60 Hz steps off a quarter-second timer leave 4.9e-17 in `f64`: not yet expired in Rust, so not in script.
    let engine = ScriptEngine::default();
    let hooks = [ScriptHook::new("expired_after", 2)];
    let source = "fn expired_after(steps, step_seconds) {\n    let timer = 0.25;\n    for step in 0..steps { timer -= step_seconds; }\n    [timer <= 0.0, !(timer > 0.0), timer == 0]\n}";
    let script = Script::compile(&engine, PATH, source, &hooks).unwrap();
    let mut timer = 0.25_f64;
    for _ in 0..15 {
        timer -= 1.0 / 60.0;
    }
    assert!(timer > 0.0);

    let mut state = ScriptState::new();
    let results =
        script.call_as::<joy_game_script::rhai::Array>(&engine, "expired_after", &mut state, (15_i64, 1.0 / 60.0));
    let results: Vec<bool> = results.unwrap().into_iter().map(|value| value.cast::<bool>()).collect();
    assert_eq!(results, vec![false, false, false]);
}

#[test]
fn a_handle_kept_in_state_cannot_reach_the_lent_value_after_its_call() {
    let mut engine = ScriptEngine::default();
    let counter: ScriptLoan<Vec<i64>> = ScriptLoan::new("counter list");
    let lent = counter.clone();
    engine
        .rhai_mut()
        .register_type_with_name::<ScriptLoan<Vec<i64>>>("Counters")
        .register_fn("push", move |_: &mut ScriptLoan<Vec<i64>>, value: i64| lent.write(|list| list.push(value)));
    let hooks = [ScriptHook::new("remember", 1), ScriptHook::new("reuse", 0)];
    let source = "fn remember(counters) { counters.push(1); this.kept = counters; }\nfn reuse() { this.kept.push(2); }";
    let script = Script::compile(&engine, PATH, source, &hooks).unwrap();
    let mut state = ScriptState::new();

    let (list, result) = counter.lend(Vec::new(), || script.call(&engine, "remember", &mut state, (counter.clone(),)));
    let _ = result.unwrap();
    assert_eq!(list, vec![1]);
    assert!(!counter.is_lent());

    let error = script.call(&engine, "reuse", &mut state, ()).unwrap_err();
    assert!(matches!(error, ScriptError::Runtime { ref message, .. } if message.contains("only available")), "{error}");
}
