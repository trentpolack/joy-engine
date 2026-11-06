// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Hook calls, per-object state, and hot reload: the contract a host's scripted objects depend on to keep their state
//! while authors edit behavior.

use joy_game_script::Script;
use joy_game_script::ScriptEngine;
use joy_game_script::ScriptError;
use joy_game_script::ScriptHook;
use joy_game_script::ScriptState;

const PATH: &str = "assets/scripts/charger.lua";
const HOOKS: [ScriptHook; 2] = [ScriptHook::new("init", 0), ScriptHook::new("tick", 1)];

const CHARGER: &str = r#"
    local RATE = 2.0
    function init(this)
        this.charge = 0.0
        this.ticks = 0
    end
    function tick(this, elapsed_seconds)
        add_charge(this, elapsed_seconds * RATE)
        this.ticks = this.ticks + 1
        return this.charge
    end
    function add_charge(this, amount)
        this.charge = this.charge + amount
    end
"#;

#[test]
fn hot_reload_keeps_object_state_and_a_broken_edit_keeps_the_previous_program() {
    let engine = ScriptEngine::default();
    let mut script = Script::compile(&engine, PATH, CHARGER, &HOOKS).unwrap();
    let mut state = ScriptState::new();
    let _ = script.call(&engine, "init", &mut state, ()).unwrap();
    let charge: f64 = script.call_as(&engine, "tick", &mut state, (0.5,)).unwrap();
    assert_eq!(charge, 1.0, "helpers receive the same state and retain their lexical constants");

    // An edit that fails to parse, or drops a hook the host calls, is refused; the running program is unchanged.
    let broken = script
        .replace_source(&engine, "function tick(this, elapsed_seconds) this.charge = this.charge + ; end")
        .unwrap_err();
    assert!(matches!(broken, ScriptError::Compile { ref location, .. } if location.path == PATH && location.line == 1));
    let missing = script.replace_source(&engine, "function init(this) end").unwrap_err();
    assert!(matches!(missing, ScriptError::MissingHook { ref name, arity: 1, .. } if name == "tick"));
    assert_eq!(script.revision(), 0);
    assert_eq!(script.call_as::<f64>(&engine, "tick", &mut state, (0.5,)).unwrap(), 2.0);

    // An accepted edit changes behavior on the next call, against the object's existing fields.
    let faster = CHARGER.replace("local RATE = 2.0", "local RATE = 10.0");
    script.replace_source(&engine, &faster).unwrap();
    assert_eq!(script.revision(), 1);
    assert_eq!(script.call_as::<f64>(&engine, "tick", &mut state, (0.5,)).unwrap(), 7.0);
    assert_eq!(state.get_as::<i64>("ticks"), Some(3));
}

#[test]
fn runtime_failures_name_the_failing_line_and_call_stack_and_keep_the_script_usable() {
    let engine = ScriptEngine::default();
    let source = "function init(this) this.charge = 0.0 end\nfunction tick(this, elapsed_seconds) local result = helper(this) return result end\nfunction helper(this)\n    return this.chrage + 1.0\nend";
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
    assert!(call_stack.contains("helper"), "{call_stack}");

    // The wrong return type is reported rather than misread.
    let error = script.call_as::<bool>(&engine, "init", &mut state, ()).unwrap_err();
    assert!(matches!(error, ScriptError::ReturnType { .. }), "{error}");
}

#[test]
fn failed_hooks_close_lua54_locals_and_preserve_the_original_error() {
    let engine = ScriptEngine::default();
    let source = "function run(this) local guard <close> = setmetatable({}, {__close = function() this.closed = true error('cleanup failure') end}) error('original failure') end";
    let script = Script::compile(&engine, PATH, source, &[ScriptHook::new("run", 0)]).unwrap();
    let mut state = ScriptState::new();
    let error = script.call(&engine, "run", &mut state, ()).unwrap_err();
    assert_eq!(state.get_as::<bool>("closed"), Some(true));
    assert!(error.to_string().contains("original failure"), "{error}");
}
