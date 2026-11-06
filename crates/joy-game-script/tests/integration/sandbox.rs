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

const PATH: &str = "sandbox.lua";

#[test]
fn a_runaway_loop_stops_at_the_instruction_budget_and_the_next_call_runs() {
    let engine = ScriptEngine::new(ScriptLimits {
        instructions_per_call: 10_000,
        ..ScriptLimits::default()
    });
    let hooks = [ScriptHook::new("spin", 1)];
    let script =
        Script::compile(&engine, PATH, "function spin(this, forever) while forever do end return 1 end", &hooks)
            .unwrap();
    let mut state = ScriptState::new();

    let error = script.call(&engine, "spin", &mut state, (true,)).unwrap_err();
    assert!(
        matches!(error, ScriptError::Runtime { ref message, .. } if message.contains("instruction budget")),
        "{error}"
    );
    assert_eq!(script.call_as::<i64>(&engine, "spin", &mut state, (false,)).unwrap(), 1);
}

#[test]
fn scripts_cannot_import_files_or_use_the_platform_power_operator() {
    let engine = ScriptEngine::default();
    let import = Script::compile(&engine, PATH, "local secrets = require(\"secrets\")", &[]).unwrap_err();
    assert!(matches!(import, ScriptError::Compile { .. }), "{import}");
    let power = Script::compile(&engine, PATH, "function grow(this, x) return x ^ 2.0 end", &[]).unwrap_err();
    assert!(matches!(power, ScriptError::Compile { .. }), "{power}");
}

#[test]
fn script_math_and_seeded_draws_match_the_rust_helpers_exactly() {
    let engine = ScriptEngine::default();
    let hooks = [ScriptHook::new("sample", 2)];
    let source = r#"
        function sample(this, heading, random)
            local direction = vec3(sin(heading), 0.0, cos(heading)) * 20.0
            local offset = direction - vec3(1.5, 0.0, -2.0)
            return {hypot(offset.x, offset.z), offset:length(), random:next_unit(), random:range(2.0, 4.0)}
        end
    "#;
    let script = Script::compile(&engine, PATH, source, &hooks).unwrap();
    let random = ScriptRandom::new();
    let mut stream = SeededRandom::new(91);
    let mut expected_stream = SeededRandom::new(91);

    // The host and the script draw from one stream, in call order.
    let host_draw = stream.next_unit();
    let mut state = ScriptState::new();
    let values: Vec<f64> = random.lend(&mut stream, || {
        let values = script.call_as::<Vec<f64>>(&engine, "sample", &mut state, (0.7, random.clone()));
        values.unwrap()
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
    let source = "function expired_after(this, steps, step_seconds) local timer = 0.25 for step = 1, steps do timer = timer - step_seconds end return {timer <= 0.0, not (timer > 0.0), timer == 0} end";
    let script = Script::compile(&engine, PATH, source, &hooks).unwrap();
    let mut timer = 0.25_f64;
    for _ in 0..15 {
        timer -= 1.0 / 60.0;
    }
    assert!(timer > 0.0);

    let mut state = ScriptState::new();
    let results = script.call_as::<Vec<bool>>(&engine, "expired_after", &mut state, (15_i64, 1.0 / 60.0));
    let results: Vec<bool> = results.unwrap();
    assert_eq!(results, vec![false, false, false]);
}

#[test]
fn a_handle_kept_in_state_cannot_reach_the_lent_value_after_its_call() {
    let engine = ScriptEngine::default();
    let counter: ScriptLoan<Vec<i64>> = ScriptLoan::new("counter list");
    let lent = counter.clone();
    engine
        .lua()
        .globals()
        .set(
            "push_counter",
            engine.lua().create_function(move |_, value: i64| lent.write(|list| list.push(value))).unwrap(),
        )
        .unwrap();
    let hooks = [ScriptHook::new("remember", 0), ScriptHook::new("reuse", 0)];
    let source =
        "function remember(this) push_counter(1) this.kept = push_counter end\nfunction reuse(this) this.kept(2) end";
    let script = Script::compile(&engine, PATH, source, &hooks).unwrap();
    let mut state = ScriptState::new();

    let (list, result) = counter.lend(Vec::new(), || script.call(&engine, "remember", &mut state, ()));
    let _ = result.unwrap();
    assert_eq!(list, vec![1]);
    assert!(!counter.is_lent());

    let error = script.call(&engine, "reuse", &mut state, ()).unwrap_err();
    assert!(matches!(error, ScriptError::Runtime { ref message, .. } if message.contains("only available")), "{error}");
}

#[test]
fn protected_calls_cannot_swallow_a_budget_and_private_globals_do_not_leak() {
    let engine = ScriptEngine::new(ScriptLimits {
        instructions_per_call: 100,
        ..ScriptLimits::default()
    });
    let script = Script::compile(
        &engine,
        PATH,
        r#"
        function spin(this, forever)
            if forever then
                while true do pcall(function() while true do end end) end
            end
            return 42
        end
    "#,
        &[ScriptHook::new("spin", 1)],
    )
    .unwrap();
    let mut state = ScriptState::new();
    assert!(script.call(&engine, "spin", &mut state, (true,)).unwrap_err().to_string().contains("instruction budget"));
    assert_eq!(script.call_as::<i64>(&engine, "spin", &mut state, (false,)).unwrap(), 42);

    let writer = Script::compile(
        &engine,
        "writer",
        "PRIVATE = 99 function run(this) return PRIVATE end",
        &[ScriptHook::new("run", 0)],
    )
    .unwrap();
    let reader = Script::compile(&engine, "reader", "function run(this) return PRIVATE == nil and getfenv == nil and os == nil and coroutine == nil and math.random == nil and loadstring == nil end", &[ScriptHook::new("run", 0)]).unwrap();
    assert_eq!(writer.call_as::<i64>(&engine, "run", &mut state, ()).unwrap(), 99);
    assert!(reader.call_as::<bool>(&engine, "run", &mut state, ()).unwrap());
}

#[test]
fn memory_limit_rejects_growth_and_releases_the_failed_coroutine() {
    let mut engine = ScriptEngine::default();
    let script = Script::compile(&engine, PATH, "function grow(this, forever) if forever then local x = {} for i = 1, 100000 do x[i] = string.rep('x', 1024) end end return 7 end", &[ScriptHook::new("grow", 1)]).unwrap();
    engine.set_limits(ScriptLimits {
        memory_bytes: engine.lua().used_memory() + (512 * 1024),
        ..ScriptLimits::default()
    });
    let mut state = ScriptState::new();
    let error = script.call(&engine, "grow", &mut state, (true,)).unwrap_err();
    assert!(error.to_string().contains("memory"), "{error}");
    engine.lua().gc_collect().unwrap();
    assert_eq!(script.call_as::<i64>(&engine, "grow", &mut state, (false,)).unwrap(), 7);
}

#[test]
fn foreign_state_arguments_and_conversion_return_errors_instead_of_panicking() {
    let first = ScriptEngine::default();
    let second = ScriptEngine::default();
    let script =
        Script::compile(&first, PATH, "function read(this, value) return value end", &[ScriptHook::new("read", 1)])
            .unwrap();
    let table = second.lua().create_table().unwrap();
    let value = joy_game_script::mlua::Value::Table(table);
    let mut foreign_state = ScriptState::from_value(value.clone()).unwrap();
    assert!(script.call(&first, "read", &mut foreign_state, (1,)).is_err());
    assert!(script.call(&first, "read", &mut ScriptState::new(), (value.clone(),)).is_err());
    assert!(joy_game_script::from_script_value::<Vec<i32>>(first.lua(), "foreign", &value).is_err());
    assert!(ScriptState::new().set(first.lua(), "value", value).is_err());
    assert!(script.call(&second, "read", &mut ScriptState::new(), (1,)).is_err());
}

#[test]
fn vector_precision_and_value_semantics_match_host_vectors() {
    let engine = ScriptEngine::default();
    let script = Script::compile(&engine, PATH, "function sample(this) local original = vec3(16777217, 0, 0) local copy = original copy = copy + vec3(1, 0, 0) return {original.x, copy.x, (2 * original):length()} end", &[ScriptHook::new("sample", 0)]).unwrap();
    assert_eq!(
        script.call_as::<Vec<f64>>(&engine, "sample", &mut ScriptState::new(), ()).unwrap(),
        vec![16777217.0, 16777218.0, 33554434.0]
    );
}

#[test]
fn unwinding_a_loan_clears_it_and_restores_an_outer_loan() {
    let loan = ScriptLoan::<i64>::new("counter");
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        loan.lend(1, || panic!("host callback panic"));
    }));
    assert!(failure.is_err());
    assert!(loan.read(|value| *value).is_err());
    let (value, ()) = loan.lend(2, || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loan.lend(3, || panic!("nested host callback panic"));
        }));
        assert_eq!(loan.read(|value| *value).unwrap(), 2);
    });
    assert_eq!(value, 2);
    assert!(!loan.is_lent());
}

#[test]
fn quoted_strings_and_comments_do_not_trigger_the_power_restriction() {
    let engine = ScriptEngine::default();
    for expression in ["2 ^ 3", "2 -- comment\n ^ 3", "2 -- comment\r ^ 3"] {
        let source = format!("function grow(this) return {expression} end");
        assert!(Script::compile(&engine, PATH, &source, &[ScriptHook::new("grow", 0)]).is_err());
    }
    let source = "--!optimize 2\n--!native\n-- ^ is harmless\nfunction label(this) return [==[caret ^]==] .. '^' end";
    let script = Script::compile(&engine, PATH, source, &[ScriptHook::new("label", 0)]).unwrap();
    assert_eq!(script.call_as::<String>(&engine, "label", &mut ScriptState::new(), ()).unwrap(), "caret ^^");
}

#[test]
fn foreign_handles_inside_host_containers_fail_before_mlua_pushes_them() {
    let first = ScriptEngine::default();
    let second = ScriptEngine::default();
    let foreign = joy_game_script::mlua::Value::Table(second.lua().create_table().unwrap());
    let script =
        Script::compile(&first, PATH, "function read(this, value) return value end", &[ScriptHook::new("read", 1)])
            .unwrap();
    let mut state = ScriptState::new();
    assert!(script.call(&first, "read", &mut state, (vec![foreign.clone()],)).is_err());
    assert!(state.set(first.lua(), "nested", vec![foreign.clone()]).is_err());
    let map = std::collections::BTreeMap::from([("nested", vec![Some(foreign)])]);
    assert!(script.call(&first, "read", &mut state, (map,)).is_err());
    assert_eq!(script.call_as::<Vec<i64>>(&first, "read", &mut state, (vec![2_i64, 3],)).unwrap(), vec![2, 3]);
}

#[test]
fn portable_math_calls_match_host_helpers_and_standard_lua_comments_are_harmless() {
    let engine = ScriptEngine::default();
    let body = "function sample(this, x) return {math.sin(0.7), math.sin(x), math.atan(0.7, 0.3)} end";
    for prefix in ["", "--!optimize 2\n", "--!native\n"] {
        let script =
            Script::compile(&engine, PATH, &format!("{prefix}{body}"), &[ScriptHook::new("sample", 1)]).unwrap();
        assert_eq!(
            script.call_as::<Vec<f64>>(&engine, "sample", &mut ScriptState::new(), (0.7,)).unwrap(),
            vec![deterministic::sin(0.7), deterministic::sin(0.7), deterministic::atan2(0.7, 0.3)]
        );
    }
    let mut library = joy_game_script::ScriptLibrary::new(engine);
    library.add_source("math_data", "--!optimize 2\nreturn {value = math.sin(0.7)}");
    assert_eq!(library.constant::<f64>("math_data", "value").unwrap(), deterministic::sin(0.7));
}

#[test]
fn a_trusted_callback_panic_does_not_leave_the_next_call_on_the_old_budget() {
    let engine = ScriptEngine::new(ScriptLimits {
        instructions_per_call: 100,
        ..ScriptLimits::default()
    });
    engine
        .lua()
        .globals()
        .set(
            "host_panic",
            engine
                .lua()
                .create_function(|_, ()| -> joy_game_script::mlua::Result<()> { panic!("trusted host callback") })
                .unwrap(),
        )
        .unwrap();
    let source = "function run(this, fail) for i = 1, 10 do end if fail then host_panic() end return 5 end";
    let script = Script::compile(&engine, PATH, source, &[ScriptHook::new("run", 1)]).unwrap();
    let mut state = ScriptState::new();
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| script.call(&engine, "run", &mut state, (true,))));
    assert!(failure.is_err());
    assert_eq!(script.call_as::<i64>(&engine, "run", &mut state, (false,)).unwrap(), 5);
}

#[test]
fn standard_lua54_syntax_integer_values_and_text_only_chunks() {
    let engine = ScriptEngine::default();
    let script = Script::compile(
        &engine,
        PATH,
        "function run(this) local n = 7 // 2 return {n, n << 2, _VERSION, math.type(n)} end",
        &[ScriptHook::new("run", 0)],
    )
    .unwrap();
    let values = script.call_as::<joy_game_script::mlua::Table>(&engine, "run", &mut ScriptState::new(), ()).unwrap();
    assert_eq!(values.get::<i64>(1).unwrap(), 3);
    assert_eq!(values.get::<i64>(2).unwrap(), 12);
    assert_eq!(values.get::<String>(3).unwrap(), "Lua 5.4");
    assert_eq!(values.get::<String>(4).unwrap(), "integer");
    assert!(Script::compile(&engine, PATH, "\u{1b}Lua", &[]).is_err());
    assert!(Script::compile(&engine, PATH, "function run(this) local n = 0 n += 1 end", &[]).is_err());
}

#[test]
fn protected_budget_calls_finish_in_a_bounded_subprocess() {
    const CHILD: &str = "JOY_LUA_BUDGET_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let mut library = joy_game_script::ScriptLibrary::new(ScriptEngine::new(ScriptLimits {
            instructions_per_call: 2_000,
            ..ScriptLimits::default()
        }));
        library.add_source("runaway", "while true do pcall(function() while true do end end) end");
        for body in [
            "while true do pcall(function() while true do end end) end",
            "while true do xpcall(function() while true do end end, function() while true do end end) end",
            "table.sort({3, 2, 1}, function() while true do pcall(function() while true do end end) end end)",
            "string.gsub('x', '.', function() while true do pcall(function() while true do end end) end end)",
            "while true do pcall(function() require('runaway') end) end",
            "local guard <close> = setmetatable({}, {__close = function() while true do pcall(function() while true do end end) end end}) while true do end",
        ] {
            let source = format!("function run(this, fail) if fail then {body} end return 9 end");
            library.add_source("runner", &source);
            library.compile_script("runner", &[ScriptHook::new("run", 1)]).unwrap();
            let mut state = ScriptState::new();
            let error = library.call("runner", "run", &mut state, (true,)).unwrap_err();
            assert!(error.to_string().contains("instruction budget"), "{body}: {error}");
            assert_eq!(library.call_as::<i64>("runner", "run", &mut state, (false,)).unwrap(), 9);
        }
        library.add_source("cleanup", "function run(this, fail) if fail then local guard <close> = setmetatable({}, {__close = function() while true do pcall(function() while true do end end) end end}) error('original failure') end return 9 end");
        library.compile_script("cleanup", &[ScriptHook::new("run", 1)]).unwrap();
        let mut state = ScriptState::new();
        let error = library.call("cleanup", "run", &mut state, (true,)).unwrap_err();
        assert!(error.to_string().contains("original failure"), "{error}");
        assert_eq!(library.call_as::<i64>("cleanup", "run", &mut state, (false,)).unwrap(), 9);
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sandbox::protected_budget_calls_finish_in_a_bounded_subprocess", "--nocapture"])
        .env(CHILD, "1")
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "protected-call subprocess failed: {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("protected calls bypassed the instruction budget");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn authored_finalizers_cannot_bypass_luas_disabled_gc_hooks() {
    let engine = ScriptEngine::default();
    let source = "function run(this) return pcall(function() setmetatable({}, {__gc = function() end}) end) end";
    let script = Script::compile(&engine, PATH, source, &[ScriptHook::new("run", 0)]).unwrap();
    assert!(!script.call_as::<bool>(&engine, "run", &mut ScriptState::new(), ()).unwrap());
}

#[test]
fn protected_calls_keep_multiple_results_nil_slots_and_handler_validation() {
    let engine = ScriptEngine::default();
    let source = r#"
        function run(this)
            local ok, first, second, third = pcall(function() return 1, nil, 3 end)
            assert(ok and first == 1 and second == nil and third == 3)
            local handled, message = xpcall(function() error('bad') end, function() return 'handled' end)
            assert(not handled and message == 'handled')
            return not pcall(function() xpcall(function() return 1 end, nil) end)
        end
    "#;
    let script = Script::compile(&engine, PATH, source, &[ScriptHook::new("run", 0)]).unwrap();
    assert!(script.call_as::<bool>(&engine, "run", &mut ScriptState::new(), ()).unwrap());
}

#[test]
fn signed_host_integers_keep_values_above_float_precision() {
    let engine = ScriptEngine::default();
    let script =
        Script::compile(&engine, PATH, "function run(this, n) return n + 1 end", &[ScriptHook::new("run", 1)]).unwrap();
    let value = 9_007_199_254_740_992_i64;
    assert_eq!(script.call_as::<i64>(&engine, "run", &mut ScriptState::new(), (value,)).unwrap(), value + 1);
}

#[test]
fn standard_math_keeps_integer_and_variadic_contracts() {
    let engine = ScriptEngine::default();
    let source = "function run(this) return math.type(math.floor(3.7)) == 'integer' and math.type(math.abs(-3)) == 'integer' and math.min(5, 4, 3) == 3 and math.max(3, 4, 5) == 5 and math.fmod(9007199254740993, 2) == 1 end";
    let script = Script::compile(&engine, PATH, source, &[ScriptHook::new("run", 0)]).unwrap();
    assert!(script.call_as::<bool>(&engine, "run", &mut ScriptState::new(), ()).unwrap());
}
