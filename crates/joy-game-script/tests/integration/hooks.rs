// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Hook calls, per-object state, and hot reload: the contract a host's scripted objects depend on to keep their state
//! while authors edit behavior.

use joy_game_script::Script;
use joy_game_script::ScriptEngine;
use joy_game_script::ScriptError;
use joy_game_script::ScriptHook;
use joy_game_script::ScriptState;

const PATH: &str = "assets/scripts/charger.rhai";
const HOOKS: [ScriptHook; 2] = [ScriptHook::new("init", 0), ScriptHook::new("tick", 1)];

const CHARGER: &str = r#"
    const RATE = 2.0;

    fn init() {
        this.charge = 0.0;
        this.ticks = 0;
    }

    fn tick(elapsed_seconds) {
        this.add_charge(elapsed_seconds * global::RATE);
        this.ticks += 1;
        this.charge
    }

    fn add_charge(amount) {
        this.charge += amount;
    }
"#;

#[test]
fn hot_reload_keeps_object_state_and_a_broken_edit_keeps_the_previous_program() {
    let engine = ScriptEngine::default();
    let mut script = Script::compile(&engine, PATH, CHARGER, &HOOKS).unwrap();
    let mut state = ScriptState::new();
    let _ = script.call(&engine, "init", &mut state, ()).unwrap();
    let charge: f64 = script.call_as(&engine, "tick", &mut state, (0.5,)).unwrap();
    assert_eq!(charge, 1.0, "helpers called as methods share `this`, and constants read through `global::`");

    // An edit that fails to parse, or drops a hook the host calls, is refused; the running program is unchanged.
    let broken = script.replace_source(&engine, "fn tick(elapsed_seconds) { this.charge += ; }").unwrap_err();
    assert!(matches!(broken, ScriptError::Compile { ref location, .. } if location.path == PATH && location.line == 1));
    let missing = script.replace_source(&engine, "fn init() {}").unwrap_err();
    assert!(matches!(missing, ScriptError::MissingHook { ref name, arity: 1, .. } if name == "tick"));
    assert_eq!(script.revision(), 0);
    assert_eq!(script.call_as::<f64>(&engine, "tick", &mut state, (0.5,)).unwrap(), 2.0);

    // An accepted edit changes behavior on the next call, against the object's existing fields.
    let faster = CHARGER.replace("const RATE = 2.0;", "const RATE = 10.0;");
    script.replace_source(&engine, &faster).unwrap();
    assert_eq!(script.revision(), 1);
    assert_eq!(script.call_as::<f64>(&engine, "tick", &mut state, (0.5,)).unwrap(), 7.0);
    assert_eq!(state.get_as::<i64>("ticks"), Some(3));
}

#[test]
fn runtime_failures_name_the_failing_line_and_call_stack_and_keep_the_script_usable() {
    let engine = ScriptEngine::default();
    let source = "fn init() { this.charge = 0.0; }\nfn tick(elapsed_seconds) { this.helper() }\nfn helper() {\n    this.chrage + 1.0\n}";
    let script = Script::compile(&engine, PATH, source, &HOOKS).unwrap();
    let mut state = ScriptState::new();
    let _ = script.call(&engine, "init", &mut state, ()).unwrap();

    // A misspelled state field is an error, not a silent `()`.
    let error = script.call(&engine, "tick", &mut state, (0.5,)).unwrap_err();
    let ScriptError::Runtime {
        location,
        call_stack,
        ..
    } = &error
    else {
        panic!("expected a runtime error, got {error}");
    };
    assert_eq!((location.path.as_str(), location.line), (PATH, 4));
    assert_eq!(call_stack, "tick → helper");

    // The wrong return type is reported rather than misread.
    let error = script.call_as::<bool>(&engine, "init", &mut state, ()).unwrap_err();
    assert!(matches!(error, ScriptError::ReturnType { .. }), "{error}");
}
