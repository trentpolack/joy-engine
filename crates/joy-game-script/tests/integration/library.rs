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
    export const RIFLE = #{ id: "rifle", damage: 12.5, pellets: 1 };
    export const PARTS = [RIFLE, #{ id: "shotgun", damage: 4.0, pellets: 8 }];

    fn part(id) {
        for part in global::PARTS {
            if part.id == id { return part; }
        }
        throw `no part ${id}`;
    }
"#;

const COMBAT: &str = r#"
    import "data/parts" as parts;

    fn volley(id) {
        let part = parts::part(id);
        part.damage * part.pellets
    }
"#;

const BATTLE: &str = r#"
    import "rules/combat" as combat;
    import "data/parts" as parts;

    fn fire(id) {
        this.fired += 1;
        combat::volley(id) + parts::PARTS.len()
    }
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
    state.set("fired", 0_i64);
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
    state.set("fired", 0_i64);

    // Data edits reach the hook script through the rules module that imports them.
    let recompiled = library.replace_source("data/parts", &PARTS.replace("damage: 4.0", "damage: 5.0")).unwrap();
    assert_eq!(recompiled, vec!["battle".to_string()]);
    assert_eq!(fire(&library, &mut state, "shotgun").unwrap(), 42.0);
    assert_eq!(library.script("battle").unwrap().revision(), 1);

    // A rules edit that breaks at its top level is refused; data, rules, and the hook script keep running as before.
    let error = library.replace_source("rules/combat", "import \"data/missing\" as missing;").unwrap_err();
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
    let value = to_script_value("unit 7", &unit).unwrap();

    // Scripts see real vectors and move them with vector math.
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library.add_source(
        "mover",
        "fn step(unit) { unit.position += vec3(0.5, 0.0, 0.0); unit.hazard = unit.position; unit }",
    );
    library.compile_script("mover", &[ScriptHook::new("step", 1)]).unwrap();
    let moved = library.call("mover", "step", &mut ScriptState::new(), (value,)).unwrap();
    let moved: Unit = from_script_value("unit 7", &moved).unwrap();

    let expected = DVec3::new((1.0 / 3.0) + 0.5, 2.0, -0.1);
    assert_eq!(moved.position, expected);
    assert_eq!(moved.hazard, Some(expected));
    assert_eq!(moved.heading.to_bits(), unit.heading.to_bits());
    assert_eq!(moved.mounts, unit.mounts);
}

#[test]
fn a_script_built_from_parts_calls_across_them_as_methods_and_reloads_with_any_part() {
    let mut library = ScriptLibrary::new(ScriptEngine::default());
    library
        .add_source("rules/unit", "const DRAG = 0.5;\nfn slow(dt) { this.speed -= this.speed * global::DRAG * dt; }");
    library.add_source(
        "battle",
        "fn step(dt) { for i in 0..this.units.len() { this.units[i].slow(dt); } this.units[0].speed }",
    );
    library.compile_script_parts("battle", &["battle", "rules/unit"], &[ScriptHook::new("step", 1)]).unwrap();

    let mut state = ScriptState::new();
    let unit = |speed: f64| {
        let mut map = joy_game_script::rhai::Map::new();
        map.insert("speed".into(), speed.into());
        joy_game_script::rhai::Dynamic::from_map(map)
    };
    state.set("units", vec![unit(10.0), unit(4.0)]);
    assert_eq!(library.call_as::<f64>("battle", "step", &mut state, (1.0,)).unwrap(), 5.0);

    let recompiled = library
        .replace_source(
            "rules/unit",
            "const DRAG = 0.25;\nfn slow(dt) { this.speed -= this.speed * global::DRAG * dt; }",
        )
        .unwrap();
    assert_eq!(recompiled, vec!["battle".to_string()]);
    assert_eq!(library.call_as::<f64>("battle", "step", &mut state, (1.0,)).unwrap(), 3.75);
}
