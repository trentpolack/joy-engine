// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Level runtime behavior: component-driven bodies and vehicles with entity scale, rollback, transform sync, handle
//! freshness, and a controller driving a kart through its behavior.

use std::cell::Cell;
use std::rc::Rc;

use glam::Vec3;
use joy_simulation::gameplay::{
    ControlIntent, EntityBehavior, EntityDefinition, EntityId, EntityRegistry, EntityState, GameplayError,
    IntentController, LevelData, LevelRuntime, PhysicsComponentError, SimulationContext, parse_level, serialize_level,
};
use joy_simulation::physics::VehicleInput;
use serde_json::json;

/// Engine-owned fixture: static ground, a scaled dynamic crate, a four-wheel kart, and a lamp.
const TEST_YARD: &str = include_str!("fixtures/test-yard.joylevel");

/// Forwards the possessed entity's control intent to its vehicle every update.
struct KartBehavior;

impl EntityBehavior for KartBehavior {
    fn update(&mut self, entity: &mut EntityState, context: &mut SimulationContext<'_>, _elapsed_seconds: f32) {
        let Some(vehicle) = context.vehicle(entity.id()) else {
            return;
        };
        let intent = &entity.control_intent;
        let input = VehicleInput {
            throttle: intent.number("throttle") as f32,
            steer: intent.number("steer") as f32,
            brake: intent.number("brake") as f32,
        };
        // Intent values were validated on delivery, so the vehicle accepts them.
        context.physics.set_vehicle_input(vehicle, input).expect("delivered intent is finite");
    }
}

/// Counts spawn and destroy hooks of plain props.
struct CountedProp {
    destroyed: Rc<Cell<u32>>,
}

impl EntityBehavior for CountedProp {
    fn on_destroy(&mut self, _entity: &mut EntityState, _context: &mut SimulationContext<'_>) {
        self.destroyed.set(self.destroyed.get() + 1);
    }
}

fn registry() -> EntityRegistry {
    let mut registry = EntityRegistry::new();
    registry.register_plain("prop").unwrap();
    registry.register("kart", |_| Box::new(KartBehavior)).unwrap();
    registry
}

fn simulate(runtime: &mut LevelRuntime, seconds: f32) {
    for _ in 0..(seconds * 60.0).round() as usize {
        runtime.step(1.0 / 60.0).unwrap();
    }
}

/// A box prop with a physics component and the given scale.
fn prop(id: &str, y: f32, mass: f32, scale: Vec3) -> EntityDefinition {
    let mut definition = EntityDefinition::from_json(&json!({
        "id": id,
        "type": "prop",
        "components": {
            "physics": {"shape": "box", "halfExtents": [0.5, 0.5, 0.5], "mass": mass},
            "custom": {"preserved": true}
        }
    }))
    .unwrap();
    definition.transform.position = Vec3::new(0.0, y, 0.0);
    definition.transform.scale = scale;
    definition
}

#[test]
fn runtime_builds_scaled_bodies_and_vehicles_and_syncs_transforms() {
    let level = parse_level(TEST_YARD).unwrap();
    let mut runtime = LevelRuntime::new(&level, registry()).unwrap();
    assert_eq!(runtime.physics().body_count(), 3, "ground, crate, and kart have physics; the lamp does not");
    assert_eq!(runtime.physics().vehicle_count(), 1);
    let crate_id = EntityId::from("crate");
    let kart = EntityId::from("kart");
    assert!(runtime.body(&EntityId::from("lamp")).is_none());

    // The crate's unit box is scaled by 1.5 once: its top face is 0.75 above its center.
    let crate_body = runtime.body(&crate_id).unwrap();
    let hit = runtime.physics().raycast(Vec3::new(3.0, 10.0, 1.5), Vec3::new(3.0, 0.0, 1.5)).unwrap();
    assert_eq!(hit.body, crate_body);
    assert!((hit.point.y - 2.75).abs() < 1e-4, "{hit:?}");
    assert!((runtime.physics().body_transform(crate_body).unwrap().rotation.y - 0.3).abs() < 1e-5);
    assert!((runtime.physics().body_mass(crate_body).unwrap() - 12.0).abs() < 1e-4);

    // Wheels start at the authored chassis-local positions.
    let vehicle = runtime.vehicle(&kart).unwrap();
    let wheels = runtime.physics().wheel_transforms(vehicle);
    assert_eq!(wheels.len(), 4);
    assert!((wheels[0].position - Vec3::new(-0.7, 0.65, -3.2)).abs().max_element() < 1e-5, "{:?}", wheels[0]);

    // After settling, the crate rests on the ground and entity transforms follow their bodies.
    simulate(&mut runtime, 2.0);
    let crate_state = &runtime.entities().get(&crate_id).unwrap().state;
    assert!((crate_state.transform.position.y - 0.75).abs() < 0.05, "{:?}", crate_state.transform);
    assert_eq!(crate_state.transform.position, runtime.physics().body_transform(crate_body).unwrap().position);
    assert_eq!(crate_state.transform.scale, Vec3::splat(1.5));
    assert_eq!(runtime.physics().grounded_wheel_count(vehicle), 4);
    assert_eq!(serialize_level(runtime.level()), TEST_YARD, "authored data is never modified");
    assert_eq!(serialize_level(&level), TEST_YARD);
}

#[test]
fn controller_intent_drives_the_kart_through_its_behavior() {
    let level = parse_level(TEST_YARD).unwrap();
    let mut runtime = LevelRuntime::new(&level, registry()).unwrap();
    let kart = EntityId::from("kart");
    let mut controller =
        IntentController::new(runtime.entities_mut(), |_, _| ControlIntent::new().with("throttle", 1.0));
    controller.possess(runtime.entities_mut(), Some(&kart)).unwrap();

    simulate(&mut runtime, 0.5);
    let start = runtime.entities().get(&kart).unwrap().state.transform.position;
    for _ in 0..120 {
        controller.update(runtime.entities_mut(), 1.0 / 60.0).unwrap();
        runtime.step(1.0 / 60.0).unwrap();
    }
    let end = runtime.entities().get(&kart).unwrap().state.transform.position;
    assert!(end.z - start.z > 2.0, "positive throttle drives the kart along +Z: {start} -> {end}");
    assert!(runtime.physics().vehicle_speed(runtime.vehicle(&kart).unwrap()) > 1.0);

    // Releasing possession clears intent, so the behavior forwards a zero throttle.
    controller.possess(runtime.entities_mut(), None).unwrap();
    assert!(runtime.entities().get(&kart).unwrap().state.control_intent.is_empty());
}

#[test]
fn uniform_vehicle_scaling_scales_wheel_geometry_and_nonuniform_scaling_rolls_back() {
    let mut kart = prop("kart", 4.0, 10.0, Vec3::splat(2.0));
    kart.kind = "kart".to_owned();
    let wheels =
        json!({"wheels": [{"position": [0.5, 0, 1], "radius": 0.3, "suspensionRestLength": 0.2, "driven": true}]});
    kart.components.insert("vehicle".to_owned(), wheels.as_object().cloned().unwrap());
    let mut level = LevelData::new("Scaled");
    level.entities.push(kart.clone());

    let mut runtime = LevelRuntime::new(&level, registry()).unwrap();
    let wheel = runtime.physics().wheel_transforms(runtime.vehicle(&EntityId::from("kart")).unwrap())[0];
    assert_eq!(wheel.position.x, 1.0);
    assert_eq!(wheel.position.z, 2.0);

    // A nonuniformly scaled vehicle fails and leaves no entity, body, or vehicle behind.
    let mut stretched = kart.clone();
    stretched.id = "stretched".to_owned();
    stretched.transform.scale = Vec3::new(2.0, 1.0, 2.0);
    let error = runtime.spawn(&stretched).unwrap_err();
    assert_eq!(
        error,
        GameplayError::PhysicsComponent {
            entity: EntityId::from("stretched"),
            source: PhysicsComponentError::NonUniformVehicleScale,
        }
    );
    assert!(error.to_string().contains("uniform"));
    assert!(!runtime.entities().contains(&EntityId::from("stretched")));
    assert_eq!(runtime.physics().body_count(), 1);
    assert_eq!(runtime.physics().vehicle_count(), 1);
}

#[test]
fn malformed_later_components_roll_back_level_setup() {
    let destroyed = Rc::new(Cell::new(0));
    let counted_registry = || {
        let mut registry = EntityRegistry::new();
        let counter = destroyed.clone();
        registry.register("prop", move |_| Box::new(CountedProp { destroyed: counter.clone() })).unwrap();
        registry
    };

    let mut linked = prop("linked", 0.0, 0.0, Vec3::ONE);
    linked.template = Some("assets/prop.joyobject".to_owned());
    linked.overrides = Some(Default::default());
    let mut unresolved = LevelData::new("Unresolved");
    unresolved.entities.push(linked);
    let error = LevelRuntime::new(&unresolved, counted_registry()).unwrap_err();
    assert_eq!(error, GameplayError::UnresolvedTemplate(EntityId::from("linked")));
    assert!(error.to_string().contains("resolve_level_objects"));

    let mut invalid = prop("invalid", 4.0, 1.0, Vec3::ONE);
    invalid.components["physics"].insert("halfExtents".to_owned(), json!([1, -2, 1]));
    let mut level = LevelData::new("Invalid");
    level.entities.push(prop("ground", -0.5, 0.0, Vec3::new(2.0, 1.0, 2.0)));
    level.entities.push(invalid);
    let error = LevelRuntime::new(&level, counted_registry()).unwrap_err();
    assert!(matches!(error, GameplayError::PhysicsComponent { source: PhysicsComponentError::InvalidHalfExtents, .. }));
    assert_eq!(destroyed.get(), 2, "the failed entity and the earlier ground were both destroyed");

    let mut vehicle_only = EntityDefinition::new("cart");
    vehicle_only.components.insert("vehicle".to_owned(), json!({"wheels": []}).as_object().cloned().unwrap());
    let mut level = LevelData::new("Vehicle only");
    level.entities.push(vehicle_only);
    let error = LevelRuntime::new(&level, EntityRegistry::new()).unwrap_err();
    assert_eq!(error.to_string(), "Entity cart: vehicle requires a physics component.");
}

#[test]
fn dynamic_replacement_receives_fresh_handles_and_stale_handles_drop() {
    let mut kart = prop("kart", 4.0, 10.0, Vec3::ONE);
    kart.components.insert(
        "vehicle".to_owned(),
        json!({"wheels": [{"position": [0, 0, 0], "driven": true}]}).as_object().cloned().unwrap(),
    );
    let mut level = LevelData::new("Replacement");
    level.entities.push(kart.clone());
    let id = EntityId::from("kart");

    for inspect_before_step in [false, true] {
        let mut runtime = LevelRuntime::new(&level, registry()).unwrap();
        let body = runtime.body(&id).unwrap();
        let vehicle = runtime.vehicle(&id).unwrap();
        assert!(runtime.remove(&id));
        assert!(!runtime.physics().contains_body(body), "removing an entity removes the body it owns");
        assert!(!runtime.physics().contains_vehicle(vehicle));

        let mut replacement = kart.clone();
        replacement.transform.position = Vec3::new(3.0, 9.0, 2.0);
        runtime.spawn(&replacement).unwrap();
        if inspect_before_step {
            assert_ne!(runtime.vehicle(&id), Some(vehicle));
            assert_ne!(runtime.body(&id), Some(body));
        }
        runtime.step(0.0).unwrap();
        assert_eq!(runtime.entities().get(&id).unwrap().state.transform.position, Vec3::new(3.0, 9.0, 2.0));
        assert_ne!(runtime.body(&id), Some(body));
        assert_ne!(runtime.vehicle(&id), Some(vehicle));
        assert_eq!(runtime.world().bodies.len(), 1);
        assert_eq!(runtime.world().vehicles.len(), 1);
        assert_eq!(runtime.physics().body_count(), 1);
        assert_eq!(runtime.physics().vehicle_count(), 1);
    }
}

#[test]
fn removing_an_entity_inside_an_update_releases_its_physics() {
    /// Removes the crate on its first update.
    struct Remover;
    impl EntityBehavior for Remover {
        fn update(&mut self, _entity: &mut EntityState, context: &mut SimulationContext<'_>, _elapsed_seconds: f32) {
            context.request_remove(EntityId::from("crate"));
        }
    }

    let mut registry = registry();
    registry.register("remover", |_| Box::new(Remover)).unwrap();
    let mut level = parse_level(TEST_YARD).unwrap();
    level.entities.push(EntityDefinition { kind: "remover".to_owned(), ..EntityDefinition::new("remover") });
    let mut runtime = LevelRuntime::new(&level, registry).unwrap();
    let crate_body = runtime.body(&EntityId::from("crate")).unwrap();

    runtime.step(1.0 / 60.0).unwrap();
    assert!(!runtime.entities().contains(&EntityId::from("crate")));
    assert!(!runtime.physics().contains_body(crate_body));
    assert!(runtime.body(&EntityId::from("crate")).is_none());
    assert!(!runtime.world().bodies.contains_key("crate"), "stale handles are dropped after the step");
    assert_eq!(runtime.physics().body_count(), 2);
}
