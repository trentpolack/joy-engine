// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Script libraries: imports between sources, authored data read into Rust records, record conversion with vectors,
//! and hot reload that follows imports and rolls back when any dependent breaks.

use joy_core::math::DVec3;
use joy_game_script::ScriptEngine;
use joy_game_script::ScriptError;
use joy_game_script::ScriptHook;
use joy_game_script::ScriptLibrary;
use joy_game_script::ScriptState;
use joy_game_script::from_script_value;
use joy_game_script::script_vec3;
use joy_game_script::to_script_value;
use serde::Deserialize;
use serde::Serialize;

const PARTS: &str = r#"
    local RIFLE = { id = "rifle", damage = 12.5, pellets = 1 }
    local PARTS = {RIFLE, { id = "shotgun", damage = 4.0, pellets = 8 }}
    local function part(id)
        for _, part in ipairs(PARTS) do
            if part.id == id then return part end
        end
        error("no part " .. id)
    end
    return {RIFLE = RIFLE, PARTS = PARTS, part = part}
"#;

const COMBAT: &str = r#"
    local parts = require("data/parts")
    local function volley(id)
        local part = parts.part(id)
        return part.damage * part.pellets
    end
    return {volley = volley}
"#;

const BATTLE: &str = r#"
    local combat = require("rules/combat")
    local parts = require("data/parts")
    function fire(this, id)
        this.fired = this.fired + 1
        return combat.volley(id) + #parts.PARTS
    end
"#;

const HOOKS: [ScriptHook; 1] = [ScriptHook::new("fire", 1)];

#[derive(Debug, Deserialize, PartialEq)]
struct Part {
    id: String,
    damage: f64,
    pellets: i64,
}

fn library() -> ScriptLibrary {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source("data/parts", PARTS);
    library.add_source("rules/combat", COMBAT);
    library.add_source("battle", BATTLE);
    library.compile_script("battle", &HOOKS).unwrap();
    library
}

fn fire(library: &ScriptLibrary, state: &mut ScriptState, id: &str) -> Result<f64, ScriptError> {
    library.call_as::<f64>("battle", "fire", state, (id.to_string(),))
}

#[test]
fn imports_share_rules_and_data_and_rust_reads_the_same_data() {
    let library = library();
    let mut state = ScriptState::new();
    state.set(library.engine().lua(), "fired", 0_i64).unwrap();
    assert_eq!(fire(&library, &mut state, "shotgun").unwrap(), 34.0);

    let parts: Vec<Part> = library.constant("data/parts", "PARTS").unwrap();
    assert_eq!(
        parts[1],
        Part {
            id: "shotgun".into(),
            damage: 4.0,
            pellets: 8
        }
    );
    let missing = library.constant::<Part>("data/parts", "LASER").unwrap_err();
    assert!(matches!(missing, ScriptError::Conversion { .. }), "{missing}");
}

#[test]
fn reloading_a_module_recompiles_its_importers_and_a_broken_edit_changes_nothing() {
    let mut library = library();
    let mut state = ScriptState::new();
    state.set(library.engine().lua(), "fired", 0_i64).unwrap();

    // Data edits reach the hook script through the rules module that imports them.
    let recompiled = library.replace_source("data/parts", &PARTS.replace("damage = 4.0", "damage = 5.0")).unwrap();
    assert_eq!(recompiled, vec!["battle".to_string()]);
    assert_eq!(fire(&library, &mut state, "shotgun").unwrap(), 42.0);
    assert_eq!(library.script("battle").unwrap().revision(), 1);

    // A rules edit that breaks at its top level is refused; data, rules, and the hook script keep running as before.
    let error = library.replace_source("rules/combat", "return require(\"data/missing\")").unwrap_err();
    assert!(matches!(error, ScriptError::Compile { .. } | ScriptError::Runtime { .. }), "{error}");
    assert_eq!(fire(&library, &mut state, "shotgun").unwrap(), 42.0);
    assert_eq!(library.source("rules/combat").as_deref(), Some(COMBAT));
    assert_eq!(state.get_as::<i64>("fired"), Some(2));
}

#[test]
fn records_round_trip_with_vectors_and_exact_floats() {
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Unit {
        id: u32,
        #[serde(with = "script_vec3")]
        position: DVec3,
        #[serde(with = "script_vec3::option")]
        hazard: Option<DVec3>,
        heading: f64,
        mounts: Vec<String>,
    }

    let unit = Unit {
        id: 7,
        position: DVec3::new(1.0 / 3.0, 2.0, -0.1),
        hazard: None,
        heading: std::f64::consts::PI,
        mounts: vec!["rifle".into()],
    };
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    let value = to_script_value(library.engine().lua(), "unit 7", &unit).unwrap();

    // Scripts see real vectors and move them with vector math.
    library.add_source(
        "mover",
        "function step(this, unit) unit.position = unit.position + vec3(0.5, 0.0, 0.0) unit.hazard = unit.position return unit end",
    );
    library.compile_script("mover", &[ScriptHook::new("step", 1)]).unwrap();
    let moved = library.call("mover", "step", &mut ScriptState::new(), (value,)).unwrap();
    let moved: Unit = from_script_value(library.engine().lua(), "unit 7", &moved).unwrap();

    let expected = DVec3::new((1.0 / 3.0) + 0.5, 2.0, -0.1);
    assert_eq!(moved.position, expected);
    assert_eq!(moved.hazard, Some(expected));
    assert_eq!(moved.heading.to_bits(), unit.heading.to_bits());
    assert_eq!(moved.mounts, unit.mounts);
}

#[test]
fn a_script_built_from_parts_calls_across_them_as_methods_and_reloads_with_any_part() {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source(
        "rules/unit",
        "local DRAG = 0.5\nfunction slow(this, dt) this.speed = this.speed - this.speed * DRAG * dt end",
    );
    library.add_source(
        "battle",
        "function step(this, dt) for _, unit in ipairs(this.units) do slow(unit, dt) end return this.units[1].speed end",
    );
    library.compile_script_parts("battle", &["battle", "rules/unit"], &[ScriptHook::new("step", 1)]).unwrap();

    let mut state = ScriptState::new();
    let units = library
        .engine()
        .lua()
        .load("return {{speed = 10.0}, {speed = 4.0}}")
        .eval::<joy_game_script::mlua::Table>()
        .unwrap();
    state.set(library.engine().lua(), "units", units).unwrap();
    assert_eq!(library.call_as::<f64>("battle", "step", &mut state, (1.0,)).unwrap(), 5.0);

    let recompiled = library
        .replace_source(
            "rules/unit",
            "local DRAG = 0.25\nfunction slow(this, dt) this.speed = this.speed - this.speed * DRAG * dt end",
        )
        .unwrap();
    assert_eq!(recompiled, vec!["battle".to_string()]);
    assert_eq!(library.call_as::<f64>("battle", "step", &mut state, (1.0,)).unwrap(), 3.75);
}

#[test]
fn nullable_slots_remain_indexed_and_cyclic_or_deep_records_fail_cleanly() {
    let engine = ScriptEngine::default();
    let original = vec![Some(3_i64), None, Some(7), None];
    let value = to_script_value(engine.lua(), "slots", &original).unwrap();
    let roundtrip: Vec<Option<i64>> = from_script_value(engine.lua(), "slots", &value).unwrap();
    assert_eq!(roundtrip, original);
    let table = value.as_table().unwrap();
    assert_eq!(table.raw_len(), 4);
    assert_eq!(table.raw_get::<joy_game_script::mlua::Value>(2).unwrap(), joy_game_script::mlua::Value::NULL);

    let cycle = engine.lua().load("local t = {} t.child = t return t").eval::<joy_game_script::mlua::Value>().unwrap();
    let error = from_script_value::<Vec<i64>>(engine.lua(), "cyclic unit", &cycle).unwrap_err();
    assert!(error.to_string().contains("cyclic"));
    let deep = engine.lua().load("local root = {} local t = root for i = 1, 10000 do local next = {} t.child = next t = next end return root").eval::<joy_game_script::mlua::Value>().unwrap();
    let error = from_script_value::<Vec<i64>>(engine.lua(), "deep unit", &deep).unwrap_err();
    assert!(error.to_string().contains("64 levels"));
}

#[test]
fn module_exports_are_deeply_frozen_even_when_tables_alias_or_cycle() {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source("catalog", "local shared = {damage = 7} local cycle = {} cycle.self = cycle return {a = shared, b = shared, cycle = cycle}");
    library.add_source(
        "reader",
        r#"
        local catalog = require("catalog")
        function read(this)
            local succeeded = pcall(function() catalog.a.damage = 99 end)
            local cycle_changed = pcall(function() catalog.cycle.self.extra = 1 end)
            return not succeeded and not cycle_changed and catalog.b.damage == 7 and catalog.a == catalog.b
        end
    "#,
    );
    library.compile_script("reader", &[ScriptHook::new("read", 0)]).unwrap();
    assert!(library.call_as::<bool>("reader", "read", &mut ScriptState::new(), ()).unwrap());
}

#[test]
fn rejected_transitive_reload_keeps_all_dependents_and_state_and_cycles_are_rejected() {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source("data", "return {power = 3}");
    library.add_source("rules", "local data = require('data') assert(data.power > 0, 'positive power required') return {power = data.power * 2}");
    library.add_source(
        "first",
        "local rules = require('rules') function tick(this) this.ticks = (this.ticks or 0) + 1 return rules.power end",
    );
    library.add_source("second", "local rules = require('rules') function tick(this) return rules.power end");
    for name in ["first", "second"] {
        library.compile_script(name, &[ScriptHook::new("tick", 0)]).unwrap();
    }
    let mut state = ScriptState::new();
    assert_eq!(library.call_as::<i64>("first", "tick", &mut state, ()).unwrap(), 6);
    assert!(library.replace_source("data", "return {power = -1}").is_err());
    for name in ["first", "second"] {
        assert_eq!(library.script(name).unwrap().revision(), 0);
        assert_eq!(library.call_as::<i64>(name, "tick", &mut state, ()).unwrap(), 6);
    }
    assert_eq!(state.get_as::<i64>("ticks"), Some(2));
    assert_eq!(library.replace_source("data", "return {power = 5}").unwrap(), vec!["first", "second"]);
    assert_eq!(library.call_as::<i64>("first", "tick", &mut state, ()).unwrap(), 10);
    assert_eq!(state.get_as::<i64>("ticks"), Some(3));
    assert!(library.replace_source("data", "return require('rules')").unwrap_err().to_string().contains("cycle"));
    assert_eq!(library.call_as::<i64>("second", "tick", &mut state, ()).unwrap(), 10);
}

#[test]
fn rejected_reload_does_not_mutate_an_unaffected_modules_captured_state() {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source("stable", "local count = 0 return {next = function() count = count + 1 return count end}");
    library.add_source("edited", "local stable = require('stable') return {value = stable.next()}");
    library.add_source("reader", "local stable = require('stable') local edited = require('edited') function read(this) return stable.next() end");
    library.compile_script("reader", &[ScriptHook::new("read", 0)]).unwrap();
    let mut state = ScriptState::new();
    assert_eq!(library.call_as::<i64>("reader", "read", &mut state, ()).unwrap(), 2);
    assert!(
        library.replace_source("edited", "local stable = require('stable') stable.next() error('reject')").is_err()
    );
    assert_eq!(library.call_as::<i64>("reader", "read", &mut state, ()).unwrap(), 3);
}

#[test]
fn repeated_part_reloads_keep_the_structural_dependency_and_reject_a_broken_part() {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source("part", "function helper(this) this.value = this.value + 1 end");
    library.add_source("main", "function tick(this) this.value = this.value or 0 helper(this) return this.value end");
    library.compile_script_parts("main", &["main", "part"], &[ScriptHook::new("tick", 0)]).unwrap();
    let mut state = ScriptState::new();
    assert_eq!(
        library.replace_source("part", "function helper(this) this.value = this.value + 2 end").unwrap(),
        vec!["main"]
    );
    assert_eq!(library.call_as::<i64>("main", "tick", &mut state, ()).unwrap(), 2);
    assert!(library.replace_source("part", "function helper(").is_err());
    assert_eq!(library.script("main").unwrap().revision(), 1);
    assert_eq!(library.call_as::<i64>("main", "tick", &mut state, ()).unwrap(), 4);
    assert_eq!(
        library.replace_source("part", "function helper(this) this.value = this.value + 3 end").unwrap(),
        vec!["main"]
    );
    assert_eq!(library.call_as::<i64>("main", "tick", &mut state, ()).unwrap(), 7);
}

#[test]
fn frozen_exports_reject_raw_writes_and_keep_iteration_and_nullable_arrays() {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source(
        "catalog",
        "local values = {3, null, 7} return {values = values, empty = {}, modify = function() values[1] = 99 end}",
    );
    library.add_source(
        "reader",
        r#"
        local catalog = require('catalog')
        function read(this)
            local values = catalog.values
            assert(not pcall(rawset, values, 1, 99))
            assert(not pcall(setmetatable, values, {}))
            assert(not pcall(table.insert, values, 99))
            assert(not pcall(catalog.modify))
            assert(getmetatable(values) == false)
            local iterator, state, key = pairs(values)
            assert(state == nil)
            local count = 0
            for _, value in pairs(values) do count = count + 1 end
            assert(count == 3 and #values == 3 and rawlen(values) == 3)
            assert(rawget(values, 1) == 3)
            local cloned = {}
            for index, value in ipairs(values) do cloned[index] = value end
            assert(cloned[2] == null)
            assert(not pcall(rawset, math, 'sin', 99))
            assert(not pcall(function() string.byte = nil end))
            assert(getmetatable('') == false)
            return values
        end
    "#,
    );
    library.compile_script("reader", &[ScriptHook::new("read", 0)]).unwrap();
    let values = library.call("reader", "read", &mut ScriptState::new(), ()).unwrap();
    assert_eq!(
        from_script_value::<Vec<Option<i64>>>(library.engine().lua(), "values", &values).unwrap(),
        vec![Some(3), None, Some(7)]
    );
    assert_eq!(library.constant::<Vec<Option<i64>>>("catalog", "values").unwrap(), vec![Some(3), None, Some(7)]);
}

#[test]
fn freezing_a_host_array_keeps_its_empty_serde_shape() {
    let engine = ScriptEngine::default();
    engine
        .lua()
        .globals()
        .set("empty_array", to_script_value(engine.lua(), "empty", &Vec::<i64>::new()).unwrap())
        .unwrap();
    let mut library = ScriptLibrary::new(engine);
    library.add_source("catalog", "return {entries = empty_array}");
    struct Shape;
    impl<'de> serde::de::Visitor<'de> for Shape {
        type Value = bool;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an empty array")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, _: A) -> Result<bool, A::Error> {
            Ok(true)
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, _: A) -> Result<bool, A::Error> {
            Ok(false)
        }
    }
    struct IsArray(bool);
    impl<'de> Deserialize<'de> for IsArray {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_any(Shape).map(IsArray)
        }
    }
    assert!(library.constant::<IsArray>("catalog", "entries").unwrap().0);
}

#[test]
fn typed_frozen_returns_read_collections_without_copying_handle_values() {
    use joy_game_script::mlua::{
        Table,
        Value,
    };
    use std::collections::BTreeMap;

    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source(
        "data",
        "local values = {1, 2, 3} return {values = values, nested = {values, values}, record = {values = values}} ",
    );
    library.add_source("reader", "local data = require('data') function read(this, name) return data[name] end");
    library.compile_script("reader", &[ScriptHook::new("read", 1)]).unwrap();
    let mut state = ScriptState::new();
    assert_eq!(library.call_as::<Vec<i64>>("reader", "read", &mut state, ("values",)).unwrap(), vec![1, 2, 3]);
    assert_eq!(
        library.call_as::<[Vec<i64>; 2]>("reader", "read", &mut state, ("nested",)).unwrap(),
        [vec![1, 2, 3], vec![1, 2, 3]]
    );
    let record = library.call_as::<BTreeMap<String, Vec<i64>>>("reader", "read", &mut state, ("record",)).unwrap();
    assert_eq!(record["values"], vec![1, 2, 3]);

    let table = library.call_as::<Table>("reader", "read", &mut state, ("values",)).unwrap();
    let value = library.call_as::<Value>("reader", "read", &mut state, ("values",)).unwrap();
    let nested = library.call_as::<Vec<Table>>("reader", "read", &mut state, ("nested",)).unwrap();
    assert_eq!(table.to_pointer(), value.as_table().unwrap().to_pointer());
    assert_eq!(table.to_pointer(), nested[0].to_pointer());
    assert_eq!(nested[0].to_pointer(), nested[1].to_pointer());
    assert_eq!(table.raw_len(), 0, "raw handles retain the frozen proxy, never its writable backing");
    assert_eq!(table.get::<i64>(1).unwrap(), 1);
    state.set(library.engine().lua(), "catalog", table).unwrap();
    assert_eq!(state.get_as::<Vec<i64>>("catalog"), Some(vec![1, 2, 3]));
}

#[test]
fn checked_host_sequences_keep_empty_array_markers_and_nullable_slots() {
    use joy_game_script::mlua::LuaSerdeExt;
    let engine = ScriptEngine::default();
    let script = joy_game_script::Script::compile(
        &engine,
        "identity",
        "function read(this, value) return value end",
        &[ScriptHook::new("read", 1)],
    )
    .unwrap();
    let mut state = ScriptState::new();
    for value in [
        script.call(&engine, "read", &mut state, (Vec::<i64>::new(),)).unwrap(),
        script.call(&engine, "read", &mut state, ([0_i64; 0],)).unwrap(),
        script.call(&engine, "read", &mut state, (&[] as &[i64],)).unwrap(),
    ] {
        assert_eq!(
            value.as_table().unwrap().metatable().unwrap().to_pointer(),
            engine.lua().array_metatable().to_pointer()
        );
        let restored: Vec<i64> = from_script_value(engine.lua(), "empty", &value).unwrap();
        assert!(restored.is_empty());
    }
    assert_eq!(
        script.call_as::<Vec<Option<i64>>>(&engine, "read", &mut state, (vec![Some(2_i64), None, Some(4)],)).unwrap(),
        vec![Some(2), None, Some(4)]
    );
}

#[test]
fn script_sources_accept_only_edits_the_host_validates() {
    use joy_game_script::{
        ScriptSource,
        ScriptSources,
    };
    const SOURCES: [ScriptSource; 2] = [
        ScriptSource::new("rules/damage", "return { scale = 2 }"),
        ScriptSource::new(
            "battle",
            "local damage = require(\"rules/damage\")\nfunction hit(this, amount) return amount * damage.scale end",
        ),
    ];
    let hooks = [ScriptHook::new("hit", 1)];
    let validate = |sources: &std::collections::BTreeMap<String, String>| {
        let mut library = joy_game_script::library_from(ScriptEngine::default(), sources);
        library.compile_script("battle", &hooks)
    };
    let mut sources = ScriptSources::new("assets/scripts", &SOURCES);
    assert_eq!(sources.path("rules/damage"), "assets/scripts/rules/damage.lua");
    assert_eq!(sources.name_for_path("assets/scripts/battle.lua"), Some("battle"));
    assert!(sources.apply_edit("assets/form/pillar.form", "", validate).is_none(), "other assets are not scripts");

    // A broken edit keeps the shipped revision; a valid one replaces it for every later library.
    let broken = sources.apply_edit("assets/scripts/rules/damage.lua", "return {", validate).unwrap();
    assert!(broken.unwrap_err().to_string().starts_with("assets/scripts/rules/damage.lua: "));
    assert_eq!(sources.get("rules/damage"), Some(SOURCES[0].shipped));
    let accepted = sources.apply_edit("assets/scripts/rules/damage.lua", "return { scale = 3 }", validate).unwrap();
    assert_eq!(accepted.unwrap(), "rules/damage");
    let mut library = sources.library(ScriptEngine::default());
    library.compile_script("battle", &hooks).unwrap();
    let mut state = ScriptState::new();
    let damage: f64 = library.call_as("battle", "hit", &mut state, (5.0,)).unwrap();
    assert_eq!(damage, 15.0);
    assert_eq!(sources.shipped("rules/damage"), Some(SOURCES[0].shipped));
}
