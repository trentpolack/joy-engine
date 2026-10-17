// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime components, system lifetimes, and controller failure paths protected by the TypeScript suite
//! (`gameplay.test.mjs`): lifecycle order, rollback of failed attachments, and intent clearing.

use std::cell::RefCell;
use std::rc::Rc;

use joy_engine::gameplay::{
    ComponentRegistry,
    ControlIntent,
    Controller,
    Entity,
    EntityBehavior,
    EntityComponent,
    EntityDefinition,
    EntityId,
    EntityRegistry,
    EntityState,
    EntitySystem,
    EntityWorld,
    GameplayError,
    IntentController,
    SimulationContext,
    World,
};
use joy_engine::physics::{
    PhysicsWorld,
    PhysicsWorldOptions,
};
use serde_json::{
    Value,
    json,
};

/// Shared, ordered lifecycle log.
type Events = Rc<RefCell<Vec<String>>>;

fn log(events: &Events, event: impl Into<String>) {
    events.borrow_mut().push(event.into());
}

fn world(entities: EntityWorld) -> World {
    World::from_parts(entities, PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap())
}

fn definition(value: Value) -> EntityDefinition {
    EntityDefinition::from_json(&value).unwrap()
}

/// A resource owned by a component; dropping it is the Rust form of the TypeScript `own(...).destroy()`.
struct Resource {
    name: &'static str,
    events: Events,
}

impl Drop for Resource {
    fn drop(&mut self) {
        log(&self.events, format!("drop:{}", self.name));
    }
}

/// Health component: records attach with the authored `current` value, owns a resource, and records detach.
struct Health {
    starting: f64,
    events: Events,
    _resource: Option<Resource>,
}

impl EntityComponent for Health {
    fn on_attach(
        &mut self,
        entity: &mut EntityState,
        _context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        log(&self.events, format!("attach:health:{}:{}", entity.id(), self.starting));
        self._resource = Some(Resource { name: "health", events: self.events.clone() });
        Ok(())
    }

    fn on_detach(&mut self, entity: &mut EntityState, _context: &mut SimulationContext<'_>) {
        let current = entity.component("health").and_then(|record| record.get("current")).and_then(Value::as_f64);
        let current = current.map_or_else(|| "none".to_owned(), |current| current.to_string());
        log(&self.events, format!("detach:health:{current}"));
    }
}

/// A second component so reverse detachment order is observable.
struct Armor {
    events: Events,
    _resource: Resource,
}

impl EntityComponent for Armor {
    fn on_attach(
        &mut self,
        _entity: &mut EntityState,
        _context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        log(&self.events, "attach:armor");
        Ok(())
    }

    fn on_detach(&mut self, _entity: &mut EntityState, _context: &mut SimulationContext<'_>) {
        log(&self.events, "detach:armor");
    }
}

/// Behavior recording spawn, update, destroy, and drop.
struct Villager {
    events: Events,
}

impl EntityBehavior for Villager {
    fn on_spawn(
        &mut self,
        entity: &mut EntityState,
        _context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        log(&self.events, format!("spawn:{}", entity.id()));
        Ok(())
    }

    fn update(&mut self, entity: &mut EntityState, _context: &mut SimulationContext<'_>, _elapsed_seconds: f32) {
        log(&self.events, format!("update:{}", entity.id()));
    }

    fn on_destroy(&mut self, entity: &mut EntityState, _context: &mut SimulationContext<'_>) {
        log(&self.events, format!("destroy:{}", entity.id()));
    }
}

impl Drop for Villager {
    fn drop(&mut self) {
        log(&self.events, "drop:villager");
    }
}

/// System that reads the health component of every match and records its lifetime hooks.
struct HealthSystem {
    events: Events,
}

impl EntitySystem for HealthSystem {
    fn required_components(&self) -> &[&str] {
        &["health"]
    }

    fn on_add(&mut self) -> Result<(), GameplayError> {
        log(&self.events, "system:add");
        Ok(())
    }

    fn update(&mut self, _elapsed_seconds: f32, matches: &mut [&mut Entity], _context: &mut SimulationContext<'_>) {
        for entity in matches.iter_mut() {
            let starting = entity.component::<Health>("health").map(|health| health.starting);
            log(&self.events, format!("system:update:{}:{starting:?}", entity.id()));
        }
    }

    fn on_remove(&mut self) {
        log(&self.events, "system:remove");
    }
}

fn villager_world(events: &Events) -> World {
    let mut registry = EntityRegistry::new();
    let behavior_events = events.clone();
    registry.register("villager", move |_| Box::new(Villager { events: behavior_events.clone() })).unwrap();
    let mut components = ComponentRegistry::new();
    let health_events = events.clone();
    components
        .register("health", move |data, _| {
            let starting = data.get("current").and_then(Value::as_f64).unwrap_or(0.0);
            Box::new(Health { starting, events: health_events.clone(), _resource: None })
        })
        .unwrap();
    let armor_events = events.clone();
    components
        .register("armor", move |_, _| {
            let resource = Resource { name: "armor", events: armor_events.clone() };
            Box::new(Armor { events: armor_events.clone(), _resource: resource })
        })
        .unwrap();
    world(EntityWorld::with_component_registry(registry, components))
}

#[test]
fn components_attach_in_authored_order_and_release_in_reverse_after_the_behavior() {
    let events: Events = Rc::default();
    let mut world = villager_world(&events);
    let id = world
        .spawn(&definition(json!({
            "id": "villager", "type": "villager",
            "components": {"health": {"current": 10}, "cosmetic": {"hat": "red"}, "armor": {}}
        })))
        .unwrap();
    assert_eq!(*events.borrow(), ["attach:health:villager:10", "attach:armor", "spawn:villager"]);

    let entity = world.entities.get(&id).unwrap();
    assert_eq!(entity.component_kinds().collect::<Vec<_>>(), ["health", "armor"]);
    assert_eq!(entity.component::<Health>("health").map(|health| health.starting), Some(10.0));
    assert!(entity.component::<Armor>("health").is_none(), "downcasts check the concrete type");
    assert!(!entity.has_component("cosmetic"), "authored records without a factory stay data");
    assert_eq!(world.entities.query(&["health", "armor"]).len(), 1);
    assert!(world.entities.query(&["cosmetic"]).is_empty());

    // Authored data stays entity-owned and visible to component hooks.
    world.entities.get_mut(&id).unwrap().state.components["health"].insert("current".into(), json!(8));
    events.borrow_mut().clear();
    assert!(world.remove(&id));
    assert_eq!(
        *events.borrow(),
        ["destroy:villager", "detach:armor", "drop:armor", "detach:health:8", "drop:health", "drop:villager"]
    );
}

#[test]
fn systems_see_runtime_components_and_release_before_entities_at_teardown() {
    let events: Events = Rc::default();
    let mut world = villager_world(&events);
    world.spawn(&definition(json!({"id": "a", "type": "villager", "components": {"health": {"current": 3}}}))).unwrap();
    world.spawn(&definition(json!({"id": "b", "type": "villager"}))).unwrap();
    world.entities.add_system(Box::new(HealthSystem { events: events.clone() })).unwrap();
    events.borrow_mut().clear();

    world.update(0.1).unwrap();
    assert_eq!(*events.borrow(), ["system:update:a:Some(3.0)", "update:a", "update:b"]);

    events.borrow_mut().clear();
    drop(world);
    assert_eq!(
        *events.borrow(),
        ["system:remove", "destroy:a", "detach:health:3", "drop:health", "drop:villager", "destroy:b", "drop:villager"]
    );
}

/// Component whose setup fails after acquiring a resource.
struct Broken {
    events: Events,
    resource: Option<Resource>,
}

impl EntityComponent for Broken {
    fn on_attach(
        &mut self,
        _entity: &mut EntityState,
        _context: &mut SimulationContext<'_>,
    ) -> Result<(), GameplayError> {
        self.resource = Some(Resource { name: "broken", events: self.events.clone() });
        Err(GameplayError::Behavior("component setup failed".into()))
    }

    fn on_detach(&mut self, _entity: &mut EntityState, _context: &mut SimulationContext<'_>) {
        log(&self.events, "detach:broken");
    }
}

#[test]
fn failed_component_attachment_rolls_back_the_complete_spawn() {
    let events: Events = Rc::default();
    let mut world = villager_world(&events);
    let broken_events = events.clone();
    world
        .entities
        .component_registry_mut()
        .register("broken", move |_, _| Box::new(Broken { events: broken_events.clone(), resource: None }))
        .unwrap();

    let error = world
        .spawn(&definition(
            json!({"id": "v", "type": "villager", "components": {"health": {"current": 1}, "broken": {}}}),
        ))
        .unwrap_err();
    assert_eq!(error, GameplayError::Behavior("component setup failed".into()));
    assert!(world.entities.get(&EntityId::from("v")).is_none());
    assert_eq!(
        *events.borrow(),
        [
            "attach:health:v:1",
            "detach:broken",
            "drop:broken",
            "destroy:v",
            "detach:health:1",
            "drop:health",
            "drop:villager"
        ],
        "the failed component detaches first, then the entity tears down without running on_spawn"
    );
}

#[test]
fn runtime_components_can_be_added_and_removed_on_live_entities() {
    let events: Events = Rc::default();
    let mut world = villager_world(&events);
    let id = world.spawn(&definition(json!({"id": "v", "type": "villager", "components": {"armor": {}}}))).unwrap();
    events.borrow_mut().clear();

    let (entities, mut context) = world.split();
    let armor =
        Box::new(Armor { events: events.clone(), _resource: Resource { name: "extra", events: events.clone() } });
    let duplicate = entities.add_component(&id, "armor", armor, &mut context).unwrap_err();
    assert_eq!(duplicate.to_string(), "Entity v already has component armor.");
    assert_eq!(*events.borrow(), ["drop:extra"], "a rejected component is dropped without hooks");

    let health = Box::new(Health { starting: 5.0, events: events.clone(), _resource: None });
    entities.add_component(&id, "health", health, &mut context).unwrap();
    assert!(entities.get(&id).unwrap().has_component("health"));
    assert!(entities.remove_component(&id, "armor", &mut context));
    assert!(!entities.remove_component(&id, "armor", &mut context));
    assert!(entities.get(&id).unwrap().state.component("armor").is_some(), "the authored record is kept");
    assert_eq!(*events.borrow(), ["drop:extra", "attach:health:v:5", "detach:armor", "drop:armor"]);

    let blank =
        entities.add_component(&id, " ", Box::new(Broken { events: events.clone(), resource: None }), &mut context);
    assert_eq!(blank, Err(GameplayError::EmptyComponentType));
    let mut registry = ComponentRegistry::new();
    registry.register("health", |_, _| Box::new(Broken { events: Rc::default(), resource: None })).unwrap();
    let again = registry.register("health", |_, _| Box::new(Broken { events: Rc::default(), resource: None }));
    assert_eq!(again.map(|_| ()), Err(GameplayError::DuplicateComponentType("health".into())));
}

/// System whose setup fails.
struct FailingSystem {
    events: Events,
}

impl EntitySystem for FailingSystem {
    fn on_add(&mut self) -> Result<(), GameplayError> {
        Err(GameplayError::Behavior("system setup failed".into()))
    }

    fn update(&mut self, _elapsed_seconds: f32, _matches: &mut [&mut Entity], _context: &mut SimulationContext<'_>) {}

    fn on_remove(&mut self) {
        log(&self.events, "failing:remove");
    }
}

/// System requiring a blank component type.
struct BlankRequirement;

impl EntitySystem for BlankRequirement {
    fn required_components(&self) -> &[&str] {
        &[""]
    }

    fn update(&mut self, _elapsed_seconds: f32, _matches: &mut [&mut Entity], _context: &mut SimulationContext<'_>) {}
}

#[test]
fn system_setup_failures_release_the_system_and_removal_runs_on_remove_once() {
    let events: Events = Rc::default();
    let mut entities = EntityWorld::default();
    let failed = entities.add_system(Box::new(FailingSystem { events: events.clone() }));
    assert_eq!(failed, Err(GameplayError::Behavior("system setup failed".into())));
    assert_eq!(*events.borrow(), ["failing:remove"]);
    assert_eq!(entities.add_system(Box::new(BlankRequirement)), Err(GameplayError::InvalidSystemRequirement));
    assert_eq!(entities.system_count(), 0);

    let system = entities.add_system(Box::new(HealthSystem { events: events.clone() })).unwrap();
    assert!(entities.remove_system(system));
    assert!(!entities.remove_system(system));
    assert_eq!(*events.borrow(), ["failing:remove", "system:add", "system:remove"]);
}

#[test]
fn controllers_validate_deltas_and_failed_samples_leave_no_active_input() {
    let mut world = world(EntityWorld::default());
    let car = world.spawn(&definition(json!({"id": "car", "type": "character"}))).unwrap();
    let entities = &mut world.entities;

    // The possession-only controller delivers nothing itself; intent arrives through receive_control.
    let remote = Controller::new(entities);
    remote.possess(entities, Some(&car)).unwrap();
    entities.receive_control(&car, ControlIntent::from_json(&json!({"throttle": 1, "boost": true})).unwrap()).unwrap();
    remote.update(entities, 0.016).unwrap();
    assert!(entities.get(&car).unwrap().state.control_intent.flag("boost"));
    assert!(matches!(remote.update(entities, f32::NAN), Err(GameplayError::InvalidDelta(_))));
    assert!(entities.get(&car).unwrap().state.control_intent.is_empty());
    remote.release(entities);
    assert_eq!(entities.get(&car).unwrap().state.possessed_by(), None);

    // A sampler failure (for example a disconnected device) propagates and clears intent.
    let connected = Rc::new(RefCell::new(true));
    let device = connected.clone();
    let mut player = IntentController::try_new(entities, move |_, _| {
        if *device.borrow() {
            Ok(ControlIntent::new().with("throttle", 1.0))
        } else {
            Err(GameplayError::Behavior("input disconnected".into()))
        }
    });
    player.possess(entities, Some(&car)).unwrap();
    player.update(entities, 0.016).unwrap();
    assert_eq!(entities.get(&car).unwrap().state.control_intent.number("throttle"), 1.0);
    *connected.borrow_mut() = false;
    assert_eq!(player.update(entities, 0.016), Err(GameplayError::Behavior("input disconnected".into())));
    assert!(entities.get(&car).unwrap().state.control_intent.is_empty());
}
