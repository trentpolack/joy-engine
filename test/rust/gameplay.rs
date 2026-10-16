// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Entity lifetime, update order, systems, possession, and controller semantics protected by the TypeScript suite.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use joy_engine::gameplay::{
    ComponentRegistry, ControlIntent, Entity, EntityBehavior, EntityComponent, EntityDefinition, EntityId,
    EntityRegistry, EntityState, EntitySystem, EntityWorld, GameplayError, IntentController, SimulationContext, World,
    WorldStep, entity_type,
};
use joy_engine::physics::{PhysicsWorld, PhysicsWorldOptions};
use serde_json::json;

fn world(registry: EntityRegistry) -> World {
    World::new(registry, PhysicsWorldOptions::default()).unwrap()
}

fn definition(id: &str, kind: &str) -> EntityDefinition {
    EntityDefinition { kind: kind.to_owned(), ..EntityDefinition::new(id) }
}

/// Counts `reset_control` calls so possession hooks are observable.
struct CountsResets(Rc<Cell<u32>>);

impl EntityBehavior for CountsResets {
    fn reset_control(&mut self, _entity: &mut EntityState) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn possession_is_exclusive_and_transfers_clear_both_sides() {
    let resets = Rc::new(Cell::new(0));
    let mut registry = EntityRegistry::new();
    let counter = resets.clone();
    registry.register("pilot", move |_| Box::new(CountsResets(counter.clone()))).unwrap();
    let mut world = world(registry);
    let first = world.spawn(&definition("first", "pilot")).unwrap();
    let second = world.spawn(&definition("second", entity_type::CHARACTER)).unwrap();
    let entities = &mut world.entities;

    let mut player =
        IntentController::new(entities, |_, _| ControlIntent::new().with("throttle", 1.0).with("brake", false));
    let mut ai = IntentController::new(entities, |_, _| ControlIntent::new().with("throttle", 0.5));
    player.possess(entities, Some(&first)).unwrap();
    player.update(entities, 0.1).unwrap();
    let intent = &entities.get(&first).unwrap().state.control_intent;
    assert_eq!(intent, &ControlIntent::new().with("throttle", 1.0).with("brake", false));

    // Possessing an owned entity displaces its controller and clears its intent.
    let before = resets.get();
    ai.possess(entities, Some(&first)).unwrap();
    assert_eq!(player.possessed(entities), None);
    assert_eq!(entities.get(&first).unwrap().state.possessed_by(), Some(ai.id()));
    assert!(entities.get(&first).unwrap().state.control_intent.is_empty());
    assert!(resets.get() > before, "the newly possessed entity runs its reset hook");

    // Moving to another entity releases the previous one.
    ai.update(entities, 0.1).unwrap();
    let before = resets.get();
    ai.possess(entities, Some(&second)).unwrap();
    assert_eq!(entities.get(&first).unwrap().state.possessed_by(), None);
    assert!(entities.get(&first).unwrap().state.control_intent.is_empty());
    assert!(resets.get() > before, "the previously possessed entity runs its reset hook");
    ai.update(entities, 0.1).unwrap();
    assert_eq!(entities.get(&second).unwrap().state.control_intent.number("throttle"), 0.5);

    // Removed entities release their controller and cannot be possessed.
    assert!(world.remove(&second));
    let entities = &mut world.entities;
    assert_eq!(ai.possessed(entities), None);
    assert_eq!(player.possess(entities, Some(&second)), Err(GameplayError::InvalidPossessionTarget));

    // Releasing a controller clears its entity; repeated release is harmless and the id is retired.
    player.possess(entities, Some(&first)).unwrap();
    player.update(entities, 0.1).unwrap();
    let player_id = player.id();
    player.release(entities);
    entities.release_controller(player_id);
    assert_eq!(entities.get(&first).unwrap().state.possessed_by(), None);
    assert!(entities.get(&first).unwrap().state.control_intent.is_empty());
    assert_eq!(entities.possess(player_id, Some(&first)), Err(GameplayError::UnknownController));
}

#[test]
fn controller_samples_replace_stale_keys_and_fail_without_leaving_active_input() {
    let mut world = world(EntityRegistry::new());
    let car = world.spawn(&definition("car", entity_type::CHARACTER)).unwrap();
    let sample = Rc::new(RefCell::new(ControlIntent::new().with("throttle", 1.0).with("brake", true)));
    let source = sample.clone();
    let entities = &mut world.entities;
    let mut controller = IntentController::new(entities, move |_, _| source.borrow().clone());
    controller.possess(entities, Some(&car)).unwrap();

    controller.update(entities, 0.016).unwrap();
    *sample.borrow_mut() = ControlIntent::new().with("throttle", 0.5);
    assert_eq!(entities.get(&car).unwrap().state.control_intent.number("throttle"), 1.0, "samples are copied");
    controller.update(entities, 0.016).unwrap();
    assert_eq!(entities.get(&car).unwrap().state.control_intent, ControlIntent::new().with("throttle", 0.5));

    *sample.borrow_mut() = ControlIntent::new().with("throttle", f64::NAN);
    assert_eq!(controller.update(entities, 0.016), Err(GameplayError::NonFiniteIntent));
    assert!(entities.get(&car).unwrap().state.control_intent.is_empty());

    *sample.borrow_mut() = ControlIntent::new().with("throttle", 1.0);
    controller.update(entities, 0.016).unwrap();
    assert!(matches!(controller.update(entities, -1.0), Err(GameplayError::InvalidDelta(_))));
    assert!(entities.get(&car).unwrap().state.control_intent.is_empty());
}

/// Records its id on update; `first` removes `second` and spawns `third` once.
struct Actor {
    events: Rc<RefCell<Vec<String>>>,
    spawned: bool,
}

impl EntityBehavior for Actor {
    fn update(&mut self, entity: &mut EntityState, context: &mut SimulationContext<'_>, _elapsed_seconds: f32) {
        self.events.borrow_mut().push(entity.id().to_string());
        if entity.id().as_str() == "first" {
            context.request_remove(EntityId::from("second"));
            if !self.spawned {
                self.spawned = true;
                context.request_spawn(definition("third", "actor"));
            }
        }
    }
}

#[test]
fn updates_follow_insertion_order_and_frame_spawns_wait_for_the_next_update() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut registry = EntityRegistry::new();
    let log = events.clone();
    registry.register("actor", move |_| Box::new(Actor { events: log.clone(), spawned: false })).unwrap();
    let mut world = world(registry);
    world.spawn(&definition("first", "actor")).unwrap();
    world.spawn(&definition("second", "actor")).unwrap();

    world.update(0.1).unwrap();
    assert_eq!(*events.borrow(), ["first"], "second was removed before its turn; third waits");
    world.update(0.1).unwrap();
    assert_eq!(*events.borrow(), ["first", "first", "third"]);
    assert_eq!(world.entities.iter().map(|entity| entity.id().as_str()).collect::<Vec<_>>(), ["first", "third"]);
}

/// Fails during spawn after acquiring state; counts its destroy hook.
struct Broken {
    destroyed: Rc<Cell<u32>>,
}

impl EntityBehavior for Broken {
    fn on_spawn(
        &mut self,
        entity: &mut EntityState,
        _context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        entity.control_intent.insert("throttle", 1.0);
        Err(GameplayError::Behavior("resource setup failed".into()))
    }

    fn on_destroy(&mut self, entity: &mut EntityState, _context: &mut SimulationContext<'_>) {
        assert!(entity.control_intent.is_empty(), "possession and intent are released before destroy");
        self.destroyed.set(self.destroyed.get() + 1);
    }
}

#[test]
fn failed_spawns_roll_back_and_invalid_definitions_change_nothing() {
    let destroyed = Rc::new(Cell::new(0));
    let mut registry = EntityRegistry::new();
    let counter = destroyed.clone();
    registry.register("broken", move |_| Box::new(Broken { destroyed: counter.clone() })).unwrap();
    assert!(matches!(
        registry.register("broken", |_| Box::new(Actor { events: Default::default(), spawned: false })),
        Err(GameplayError::DuplicateEntityType(_))
    ));
    assert!(matches!(registry.register_plain(entity_type::ENTITY), Err(GameplayError::DuplicateEntityType(_))));
    let mut world = world(registry);

    let error = world.spawn(&definition("broken", "broken")).unwrap_err();
    assert_eq!(error, GameplayError::Behavior("resource setup failed".into()));
    assert!(world.entities.get(&EntityId::from("broken")).is_none());
    assert_eq!(destroyed.get(), 1);

    assert_eq!(world.spawn(&definition("x", "missing")), Err(GameplayError::UnknownEntityType("missing".into())));
    world.spawn(&definition("healthy", entity_type::ENTITY)).unwrap();
    assert!(matches!(world.spawn(&definition("healthy", entity_type::ENTITY)), Err(GameplayError::DuplicateEntity(_))));
    assert!(matches!(world.spawn(&definition(" ", entity_type::ENTITY)), Err(GameplayError::Definition(_))));
    assert_eq!(world.entities.len(), 1);
    drop(world);
    assert_eq!(destroyed.get(), 1, "only the failed spawn was destroyed");
}

/// One recorded system update: elapsed seconds and matching entity ids.
type SystemUpdate = (f32, Vec<String>);

/// Records matching entity ids and spawns a late marker once.
struct MarkerSystem {
    updates: Rc<RefCell<Vec<SystemUpdate>>>,
}

impl EntitySystem for MarkerSystem {
    fn required_components(&self) -> &[&str] {
        &["marker"]
    }

    fn update(&mut self, elapsed_seconds: f32, matches: &mut [&mut Entity], context: &mut SimulationContext<'_>) {
        let ids = matches.iter().map(|entity| entity.id().to_string()).collect();
        let first_frame = self.updates.borrow().is_empty();
        self.updates.borrow_mut().push((elapsed_seconds, ids));
        if first_frame {
            let late = EntityDefinition::from_json(&json!({"id": "late", "components": {"marker": {}}})).unwrap();
            context.request_spawn(late);
        }
    }
}

/// Runtime marker component with no behavior of its own.
struct Marker;

impl EntityComponent for Marker {}

#[test]
fn systems_match_components_in_stable_order_and_defer_frame_spawns() {
    let mut components = ComponentRegistry::new();
    components.register("marker", |_, _| Box::new(Marker)).unwrap();
    let entities = EntityWorld::with_component_registry(EntityRegistry::new(), components);
    let mut world = World::from_parts(entities, PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap());
    world.spawn(&EntityDefinition::from_json(&json!({"id": "first", "components": {"marker": {}}})).unwrap()).unwrap();
    world.spawn(&EntityDefinition::new("ignored")).unwrap();
    let authored_only = EntityDefinition::from_json(&json!({"id": "authored", "components": {"other": {}}})).unwrap();
    world.spawn(&authored_only).unwrap();
    let updates = Rc::new(RefCell::new(Vec::new()));
    let system = world.entities.add_system(Box::new(MarkerSystem { updates: updates.clone() })).unwrap();

    assert_eq!(
        world.entities.query(&["marker"]).iter().map(|entity| entity.id().as_str()).collect::<Vec<_>>(),
        ["first"]
    );
    world.update(0.25).unwrap();
    world.update(0.5).unwrap();
    assert_eq!(
        *updates.borrow(),
        vec![(0.25, vec!["first".to_owned()]), (0.5, vec!["first".to_owned(), "late".to_owned()])]
    );
    assert!(world.entities.remove_system(system));
    assert_eq!(world.entities.system_count(), 0);
}

#[test]
fn world_steps_physics_before_entities_and_rejects_invalid_deltas() {
    let options = PhysicsWorldOptions { fixed_step: 0.25, max_sub_steps: 2, ..Default::default() };
    let mut world = World::new(EntityRegistry::new(), options).unwrap();
    assert_eq!(world.update(0.5).unwrap(), WorldStep { elapsed_seconds: 0.5, physics_steps: 2 });
    assert!(matches!(world.update(f32::NAN), Err(GameplayError::InvalidDelta(_))));
    assert!(matches!(world.update(-1.0), Err(GameplayError::InvalidDelta(_))));
}
