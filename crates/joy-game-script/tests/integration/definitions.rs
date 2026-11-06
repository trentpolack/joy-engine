// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The editor API follows the callbacks that actually execute, and refuses incomplete registrations.

use joy_game_script::{
    LuaSignature,
    ScriptEngine,
};

#[test]
fn definitions_follow_live_callbacks_and_reject_untracked_globals() {
    let mut engine = ScriptEngine::default();
    engine.bindings().function("twice", LuaSignature::new(&["value"]), |_, value: f64| Ok(value * 2.0)).unwrap();
    assert_eq!(engine.lua().load("return twice(21)").eval::<f64>().unwrap(), 42.0);
    let output = engine.lua_definitions().unwrap();
    // Definitions are valid Lua syntax but must never execute in the gameplay VM.
    engine.lua().load(&output).into_function().unwrap();
    assert_eq!(output, engine.lua_definitions().unwrap());
    engine.lua().globals().set("forgotten", 1).unwrap();
    assert!(engine.lua_definitions().unwrap_err().to_string().contains("forgotten"));
}

#[test]
fn incorrect_parameter_metadata_cannot_install_a_callback() {
    let mut engine = ScriptEngine::default();
    let result = engine.bindings().function("bad", LuaSignature::new(&[]), |_, value: f64| Ok(value));
    assert!(result.is_err());
    assert!(engine.lua().globals().get::<Option<joy_game_script::mlua::Function>>("bad").unwrap().is_none());
}

#[test]
fn frozen_namespace_registration_is_visible_and_raw_shadows_are_rejected() {
    let mut engine = ScriptEngine::default();
    engine.bindings().function("math.extra", LuaSignature::new(&["value"]), |_, value: f64| Ok(value + 1.0)).unwrap();
    assert_eq!(engine.lua().load("return math.extra(4)").eval::<f64>().unwrap(), 5.0);
    engine.lua_definitions().unwrap();
    let math = engine.lua().globals().get::<joy_game_script::mlua::Table>("math").unwrap();
    math.raw_set("sin", engine.lua().create_function(|_, _: f64| Ok(42)).unwrap()).unwrap();
    assert_eq!(engine.lua().load("return math.sin(5)").eval::<i64>().unwrap(), 42);
    assert!(engine.lua_definitions().unwrap_err().to_string().contains("math.sin"));
}

#[test]
fn allocation_ceiling_is_applied_after_all_constructor_bookkeeping() {
    let engine = ScriptEngine::new(joy_game_script::ScriptLimits {
        memory_bytes: 1,
        ..Default::default()
    });
    assert_eq!(engine.limits().memory_bytes, 1);
}
