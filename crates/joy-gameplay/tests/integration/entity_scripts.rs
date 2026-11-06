// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Entity scripts through `ScriptSystem`: a `script` component runs its script's hooks against the world view, the
//! system applies the commands it returns (pushing bodies, timers, and events), state survives a hot reload, and a
//! failing script is reported once while its entity idles.

use std::cell::RefCell;
use std::rc::Rc;

use joy_core::math::Vec3;
use joy_game_script::{
    ScriptLibrary,
    ScriptLimits,
};
use joy_gameplay::{
    ENTITY_SCRIPT_HOOKS,
    EntityDefinition,
    EntityId,
    EntityRegistry,
    LevelData,
    LevelRuntime,
    ScriptEvent,
    ScriptFailure,
    ScriptSystem,
    entity_script_engine,
};
use serde_json::json;

/// Seeks the nearest beacon, pulses every quarter second, and reports each pulse with its count.
const SEEKER: &str = r#"
function init(this, entity)
    this.pulses = 0
    this.pulsing = false
    this.speed = entity:component("script").speed
end

function update(this, entity, world, elapsed_seconds)
    if not this.pulsing then
        world:start_timer("pulse", 0.25, true)
        this.pulsing = true
    end
    if world:timer_fired("pulse") then
        this.pulses = this.pulses + 1
        world:send("pulsed", { count = this.pulses })
    end
    local target = world:nearest(entity.position, 50.0, "beacon")
    if target ~= nil then
        local heading = world:position(target) - entity.position
        heading = vec3(heading.x, 0.0, heading.z)
        if heading:length() > 0.5 then
            world:set_velocity(heading / heading:length() * this.speed + vec3(0.0, entity.velocity.y, 0.0))
        end
    end
end
"#;

fn entity(id: &str, x: f32, components: serde_json::Value, tags: &[&str]) -> EntityDefinition {
    let mut definition = EntityDefinition::from_json(&json!({
        "id": id,
        "type": "prop",
        "tags": tags,
        "components": components
    }))
    .unwrap();
    definition.transform.position = Vec3::new(x, 0.5, 0.0);
    definition
}

fn level() -> LevelData {
    let mut level = LevelData::new("Script Yard");
    let mut ground = entity("ground", 0.0, json!({"physics": {"shape": "plane"}}), &[]);
    ground.transform.position = Vec3::ZERO;
    level.entities.push(ground);
    level.entities.push(entity("beacon", 6.0, json!({}), &["beacon"]));
    level.entities.push(entity(
        "seeker",
        0.0,
        json!({
            "physics": {"shape": "sphere", "radius": 0.5, "mass": 1.0, "friction": 0.0},
            "script": {"script": "seeker", "speed": 2.0}
        }),
        &[],
    ));
    level
}

#[test]
fn scripts_drive_bodies_through_commands_and_keep_state_across_hot_reload() {
    let library = Rc::new(RefCell::new(ScriptLibrary::new(entity_script_engine(ScriptLimits::default()))));
    library.borrow_mut().add_source("seeker", SEEKER);
    library.borrow_mut().compile_script("seeker", &ENTITY_SCRIPT_HOOKS).unwrap();

    let mut registry = EntityRegistry::new();
    registry.register_plain("prop").unwrap();
    let mut runtime = LevelRuntime::new(&level(), registry).unwrap();
    runtime.entities_mut().add_system(Box::new(ScriptSystem::new(Rc::clone(&library)))).unwrap();

    // A timer started in the first update counts from the second, fires 15 updates later, and reaches the script one
    // update after that (events reach behaviors in the next update), so pulses land on updates 17, 32, 47, and 62.
    let mut pulses = Vec::new();
    for _ in 0..62 {
        runtime.step(1.0 / 60.0).unwrap();
        for event in runtime.world().events().read::<ScriptEvent>() {
            assert_eq!((event.name.as_str(), event.sender.as_str()), ("pulsed", "seeker"));
            pulses.push(event.data["count"].as_i64().unwrap());
        }
    }
    assert_eq!(pulses, vec![1, 2, 3, 4]);
    let x = runtime.entities().get(&EntityId::from("seeker")).unwrap().state.transform.position.x;
    assert!((1.0..3.0).contains(&x), "the script pushed the body toward the beacon: {x}");

    // A hot reload keeps every entity's state: the count continues instead of restarting.
    let faster = SEEKER.replace("this.pulses = this.pulses + 1", "this.pulses = this.pulses + 10");
    library.borrow_mut().replace_source("seeker", &faster).unwrap();
    pulses.clear();
    for _ in 0..15 {
        runtime.step(1.0 / 60.0).unwrap();
        for event in runtime.world().events().read::<ScriptEvent>() {
            pulses.push(event.data["count"].as_i64().unwrap());
        }
    }
    assert_eq!(pulses, vec![14]);
}

#[test]
fn a_failing_script_is_reported_once_and_its_entity_idles() {
    let library = Rc::new(RefCell::new(ScriptLibrary::new(entity_script_engine(ScriptLimits::default()))));
    library.borrow_mut().add_source(
        "seeker",
        "function init(this, entity) end function update(this, entity, world, elapsed) this.missing = this.missing + 1 end",
    );
    library.borrow_mut().compile_script("seeker", &ENTITY_SCRIPT_HOOKS).unwrap();
    let mut registry = EntityRegistry::new();
    registry.register_plain("prop").unwrap();
    let mut runtime = LevelRuntime::new(&level(), registry).unwrap();

    runtime.entities_mut().add_system(Box::new(ScriptSystem::new(Rc::clone(&library)))).unwrap();
    let mut failures = Vec::new();
    for _ in 0..10 {
        runtime.step(1.0 / 60.0).unwrap();
        failures.extend(runtime.world().events().read::<ScriptFailure>().iter().cloned());
    }
    assert_eq!(failures.len(), 1, "the same failure is reported once: {failures:?}");
    assert_eq!(failures[0].entity.as_str(), "seeker");
    assert!(runtime.entities().contains(&EntityId::from("seeker")), "a script failure never removes its entity");
}
