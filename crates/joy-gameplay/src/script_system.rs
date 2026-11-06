// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scripted entities: an authored `script` component names a game script whose hooks drive the entity, run by a
//! [`ScriptSystem`] against a read-only view of the world. Scripts never mutate simulation state directly: they return
//! commands (spawn, remove, send an event, start a timer, push a body) that the system applies after each hook, so
//! Rust owns every structural change and the update order stays the world's.
//!
//! Every entity script defines [`ENTITY_SCRIPT_HOOKS`]:
//!
//! ```rhai
//! fn init(entity) {
//!     this.speed = entity.component("script").speed;
//! }
//!
//! fn update(entity, world, elapsed_seconds) {
//!     let target = world.nearest(entity.position, 12.0, "player");
//!     if target != () {
//!         let heading = world.position(target) - entity.position;
//!         world.set_velocity(heading / heading.length() * this.speed);
//!     }
//!     if world.timer_fired("attack") { world.send("attacked", #{ target: target }); }
//! }
//! ```
//!
//! The world view describes the state at the start of the system's update: every entity's id, kind, tags, and
//! position, plus the events delivered to this update (timers that fired for the entity, contacts it began, and
//! [`ScriptEvent`]s). It is captured once per update and shared by every script, so queries cost no physics access.
//!
//! Build the engine with [`entity_script_engine`] (it registers the `Entity` and `World` types), keep the
//! [`ScriptLibrary`] behind a shared handle so the game can hot reload sources, and add the system to the entity world.
//! A script that fails is reported once per distinct message (through the log and as a [`ScriptFailure`] event) and
//! its entity idles for that update.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{
    HashMap,
    HashSet,
};
use std::rc::Rc;

use joy_core::JsonRecord;
use joy_core::math::DVec3;
use joy_game_script::rhai::{
    Array,
    Dynamic,
    Map,
};
use joy_game_script::{
    ScriptEngine,
    ScriptHook,
    ScriptLibrary,
    ScriptLimits,
    ScriptState,
    from_script_value,
    to_script_value,
};
use joy_physics::ContactPhase;
use serde_json::Value;

use super::component::EntityComponent;
use super::component_vocabulary::component_type;
use super::entity::{
    Entity,
    EntityId,
    EntityState,
    SimulationContext,
};
use super::entity_definition::EntityDefinition;
use super::entity_system::EntitySystem;
use super::world_events::{
    ContactEvent,
    TimerFired,
};
use super::world_timers::TimerRepeat;

/// Hooks every entity script defines: `init(entity)` once before its first update, and
/// `update(entity, world, elapsed_seconds)` every world update.
pub const ENTITY_SCRIPT_HOOKS: [ScriptHook; 2] = [ScriptHook::new(INIT_HOOK, 1), ScriptHook::new(UPDATE_HOOK, 3)];

/// Field of the `script` component naming the library script.
pub const SCRIPT_NAME_FIELD: &str = "script";

const INIT_HOOK: &str = "init";
const UPDATE_HOOK: &str = "update";

/// Script type names of the views.
const ENTITY_TYPE_NAME: &str = "Entity";
const WORLD_TYPE_NAME: &str = "World";

/// Most distinct failure messages remembered, so a script failing differently every update cannot grow memory.
const MAXIMUM_REPORTED_FAILURES: usize = 64;

// ---------------------------------------------------------------------------------------------------------------------
// Public Records
// ---------------------------------------------------------------------------------------------------------------------

/// An event a script sent with `world.send(name, data)`; scripts read them with `world.events(name)` during the next
/// update, and Rust reads them as `events.read::<ScriptEvent>()`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptEvent {
    /// The script's name for the event.
    pub name: String,
    /// The entity whose script sent it.
    pub sender: EntityId,
    /// The script's data, as a JSON record.
    pub data: JsonRecord,
}

/// A script hook or command failed; sent once per distinct message so a script failing every update does not flood the
/// event stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFailure {
    /// The entity whose script failed.
    pub entity: EntityId,
    /// What failed, leading with the script location when the language reported one.
    pub message: String,
}

/// The runtime component an authored `script` record attaches: the script's name and the entity's script state, which
/// survives hot reloads.
#[derive(Debug, Default)]
pub struct ScriptComponent {
    script: String,
    state: Option<ScriptState>,
}

impl ScriptComponent {
    /// The library script this entity runs.
    pub fn script(&self) -> &str {
        &self.script
    }

    /// The entity's script state (`this`), once `init` has run.
    pub fn state(&self) -> Option<&ScriptState> {
        self.state.as_ref()
    }
}

impl EntityComponent for ScriptComponent {
    fn on_attach(
        &mut self,
        entity: &mut EntityState,
        _context: &mut SimulationContext<'_>,
    ) -> Result<(), super::GameplayError> {
        let name = entity
            .component(component_type::SCRIPT)
            .and_then(|record| record.get(SCRIPT_NAME_FIELD))
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.script = name.to_owned();
        Ok(())
    }
}

/// A game script engine with the entity and world views registered; create the entity scripts' [`ScriptLibrary`] from
/// it.
pub fn entity_script_engine(limits: ScriptLimits) -> ScriptEngine {
    let mut engine = ScriptEngine::new(limits);
    register_views(&mut engine);
    engine
}

// ---------------------------------------------------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------------------------------------------------

/// Runs the hooks of every entity carrying a `script` component, in entity order, and applies their commands.
pub struct ScriptSystem {
    library: Rc<RefCell<ScriptLibrary>>,
    // Failures to send at the end of the current update, and every message already reported.
    pending_failures: Vec<ScriptFailure>,
    reported: HashSet<String>,
}

impl ScriptSystem {
    /// A system running scripts from `library`, shared with the game so it can add and hot reload sources. Compile each
    /// entity script with [`ENTITY_SCRIPT_HOOKS`].
    pub fn new(library: Rc<RefCell<ScriptLibrary>>) -> Self {
        Self {
            library,
            pending_failures: Vec::new(),
            reported: HashSet::new(),
        }
    }

    /// Run one entity's hooks, returning the commands its update issued.
    fn run_entity(
        &mut self,
        entity: &mut Entity,
        snapshot: &Rc<WorldSnapshot>,
        velocity: DVec3,
        elapsed_seconds: f32,
    ) -> Vec<ScriptCommand> {
        let view = EntityView::capture(&entity.state, velocity);
        let Some(component) = entity.component_mut::<ScriptComponent>(component_type::SCRIPT) else {
            return Vec::new();
        };
        let library = self.library.borrow();
        let script = component.script.clone();

        // Initialize the state on the first update (and again if an earlier init failed).
        if component.state.is_none() {
            let mut state = ScriptState::new();
            if let Err(error) = library.call(&script, INIT_HOOK, &mut state, (view.clone(),)) {
                drop(library);
                self.report(&view.id, error.to_string());
                return Vec::new();
            }
            component.state = Some(state);
        }

        let commands = Rc::new(RefCell::new(Vec::new()));
        let world = WorldView {
            snapshot: Rc::clone(snapshot),
            entity: view.id.clone(),
            commands: Rc::clone(&commands),
        };
        let Some(state) = component.state.as_mut() else {
            return Vec::new();
        };
        let result = library.call(&script, UPDATE_HOOK, state, (view.clone(), world, f64::from(elapsed_seconds)));
        drop(library);
        if let Err(error) = result {
            self.report(&view.id, error.to_string());
            return Vec::new();
        }
        commands.take()
    }

    /// Log a failure and queue its event, once per distinct entity and message.
    fn report(&mut self, entity: &EntityId, message: String) {
        let key = format!("{entity}: {message}");
        if self.reported.contains(&key) || self.reported.len() >= MAXIMUM_REPORTED_FAILURES {
            return;
        }
        log::warn!("entity script failed: {key}");
        self.reported.insert(key);
        self.pending_failures.push(ScriptFailure {
            entity: entity.clone(),
            message,
        });
    }
}

impl EntitySystem for ScriptSystem {
    fn update(&mut self, elapsed_seconds: f32, matches: &mut [&mut Entity], context: &mut SimulationContext<'_>) {
        let snapshot = Rc::new(WorldSnapshot::capture(matches, context));
        for entity in matches.iter_mut() {
            if !entity.has_component(component_type::SCRIPT) {
                continue;
            }
            let id = entity.id().clone();
            let velocity = context
                .body(&id)
                .and_then(|body| context.physics.body_velocity(body))
                .map_or(DVec3::ZERO, |velocity| velocity.as_dvec3());
            let commands = self.run_entity(entity, &snapshot, velocity, elapsed_seconds);
            for command in commands {
                if let Err(message) = apply_command(&id, command, context) {
                    self.report(&id, message);
                }
            }
        }
        for failure in self.pending_failures.drain(..) {
            context.send_event(failure);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------------------------------------

/// A change a script asked for; applied by the system after the hook returns.
#[derive(Clone, Debug)]
enum ScriptCommand {
    Spawn(Value),
    Remove(String),
    Send { name: String, data: Value },
    StartTimer { key: String, seconds: f64, repeat: bool },
    SetVelocity(DVec3),
    ApplyImpulse(DVec3),
    MoveTo(DVec3),
}

/// Apply one command for the entity whose script issued it.
fn apply_command(id: &EntityId, command: ScriptCommand, context: &mut SimulationContext<'_>) -> Result<(), String> {
    match command {
        ScriptCommand::Spawn(value) => {
            let definition = EntityDefinition::from_json(&value).map_err(|error| format!("spawn: {error}"))?;
            context.request_spawn(definition);
        }
        ScriptCommand::Remove(target) => context.request_remove(EntityId::new(target)),
        ScriptCommand::Send {
            name,
            data,
        } => {
            let data = match data {
                Value::Object(record) => record,
                Value::Null => JsonRecord::new(),
                _ => return Err(format!("send {name}: data must be a map")),
            };
            context.send_event(ScriptEvent {
                name,
                sender: id.clone(),
                data,
            });
        }
        ScriptCommand::StartTimer {
            key,
            seconds,
            repeat,
        } => {
            let repeat = if repeat { TimerRepeat::Every } else { TimerRepeat::Once };
            context.start_timer(Some(id.clone()), Cow::Owned(key), seconds, repeat);
        }
        ScriptCommand::SetVelocity(velocity) => {
            let body = context.body(id).ok_or("set_velocity: the entity has no body")?;
            context.physics.set_body_velocity(body, velocity.as_vec3()).map_err(|error| error.to_string())?;
        }
        ScriptCommand::ApplyImpulse(impulse) => {
            let body = context.body(id).ok_or("apply_impulse: the entity has no body")?;
            context.physics.apply_impulse(body, impulse.as_vec3(), None).map_err(|error| error.to_string())?;
        }
        ScriptCommand::MoveTo(position) => {
            let body = context.body(id).ok_or("move_to: the entity has no body")?;
            context
                .physics
                .set_body_transform(body, Some(position.as_vec3()), None)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------------------------------

/// One entity in the world view.
#[derive(Clone, Debug)]
struct SnapshotEntity {
    id: EntityId,
    kind: String,
    tags: Vec<String>,
    position: DVec3,
}

/// The world as scripts see it during one system update.
#[derive(Debug, Default)]
struct WorldSnapshot {
    entities: Vec<SnapshotEntity>,
    index: HashMap<EntityId, usize>,
    timers: HashMap<EntityId, Vec<String>>,
    contacts: HashMap<EntityId, Vec<EntityId>>,
    events: Vec<(String, EntityId, Value)>,
}

impl WorldSnapshot {
    /// Capture every entity and the events delivered to this update.
    fn capture(entities: &[&mut Entity], context: &SimulationContext<'_>) -> Self {
        let mut snapshot = Self::default();
        for entity in entities {
            snapshot.index.insert(entity.id().clone(), snapshot.entities.len());
            snapshot.entities.push(SnapshotEntity {
                id: entity.id().clone(),
                kind: entity.state.kind.clone(),
                tags: entity.state.tags.clone(),
                position: entity.state.transform.position.as_dvec3(),
            });
        }
        let delivered = context.events();
        for timer in delivered.read::<TimerFired>() {
            if let Some(owner) = &timer.owner {
                snapshot.timers.entry(owner.clone()).or_default().push(timer.key.to_string());
            }
        }
        for contact in delivered.read::<ContactEvent>() {
            if contact.phase != ContactPhase::Started {
                continue;
            }
            if let (Some(first), Some(second)) = (&contact.first, &contact.second) {
                snapshot.contacts.entry(first.clone()).or_default().push(second.clone());
                snapshot.contacts.entry(second.clone()).or_default().push(first.clone());
            }
        }
        for event in delivered.read::<ScriptEvent>() {
            snapshot.events.push((event.name.clone(), event.sender.clone(), Value::Object(event.data.clone())));
        }
        snapshot
    }

    /// Ids of entities within `radius` of `center` carrying `tag` (any entity for an empty tag), nearest first, ties
    /// in entity order.
    fn within(&self, center: DVec3, radius: f64, tag: &str) -> Vec<&SnapshotEntity> {
        let mut found: Vec<(f64, usize, &SnapshotEntity)> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, entity)| tag.is_empty() || entity.tags.iter().any(|candidate| candidate == tag))
            .map(|(order, entity)| (entity.position.distance(center), order, entity))
            .filter(|(distance, _, _)| *distance <= radius)
            .collect();
        found.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
        found.into_iter().map(|(_, _, entity)| entity).collect()
    }
}

/// The entity a hook runs for, passed by value: id, kind, position, velocity, tags, and authored records.
#[derive(Clone, Debug)]
struct EntityView {
    id: EntityId,
    kind: String,
    position: DVec3,
    velocity: DVec3,
    tags: Rc<[String]>,
    components: Rc<JsonRecord>,
}

impl EntityView {
    fn capture(state: &EntityState, velocity: DVec3) -> Self {
        let components: JsonRecord =
            state.components.iter().map(|(kind, record)| (kind.clone(), Value::Object(record.clone()))).collect();
        Self {
            id: state.id().clone(),
            kind: state.kind.clone(),
            position: state.transform.position.as_dvec3(),
            velocity,
            tags: state.tags.clone().into(),
            components: Rc::new(components),
        }
    }
}

/// The world a hook runs in: queries over the captured snapshot plus the entity's command list.
#[derive(Clone, Debug)]
struct WorldView {
    snapshot: Rc<WorldSnapshot>,
    entity: EntityId,
    commands: Rc<RefCell<Vec<ScriptCommand>>>,
}

impl WorldView {
    fn push(&self, command: ScriptCommand) {
        self.commands.borrow_mut().push(command);
    }
}

/// Register the `Entity` and `World` types and their methods.
fn register_views(engine: &mut ScriptEngine) {
    let rhai = engine.rhai_mut();
    rhai.register_type_with_name::<EntityView>(ENTITY_TYPE_NAME)
        .register_get("id", |entity: &mut EntityView| entity.id.as_str().to_owned())
        .register_get("kind", |entity: &mut EntityView| entity.kind.clone())
        .register_get("position", |entity: &mut EntityView| entity.position)
        .register_get("velocity", |entity: &mut EntityView| entity.velocity)
        .register_fn("has_tag", |entity: &mut EntityView, tag: &str| {
            entity.tags.iter().any(|candidate| candidate == tag)
        })
        .register_fn("component", |entity: &mut EntityView, kind: &str| -> Dynamic {
            entity.components.get(kind).and_then(|record| to_script_value(kind, record).ok()).unwrap_or(Dynamic::UNIT)
        });

    rhai.register_type_with_name::<WorldView>(WORLD_TYPE_NAME)
        .register_fn("position", |world: &mut WorldView, id: &str| -> Dynamic {
            world
                .snapshot
                .index
                .get(id)
                .and_then(|index| world.snapshot.entities.get(*index))
                .map_or(Dynamic::UNIT, |entity| Dynamic::from(entity.position))
        })
        .register_fn("kind", |world: &mut WorldView, id: &str| -> Dynamic {
            world
                .snapshot
                .index
                .get(id)
                .and_then(|index| world.snapshot.entities.get(*index))
                .map_or(Dynamic::UNIT, |entity| Dynamic::from(entity.kind.clone()))
        })
        .register_fn("within", |world: &mut WorldView, center: DVec3, radius: f64, tag: &str| -> Array {
            world
                .snapshot
                .within(center, radius, tag)
                .into_iter()
                .filter(|entity| entity.id != world.entity)
                .map(|entity| Dynamic::from(entity.id.as_str().to_owned()))
                .collect()
        })
        .register_fn("nearest", |world: &mut WorldView, center: DVec3, radius: f64, tag: &str| -> Dynamic {
            world
                .snapshot
                .within(center, radius, tag)
                .into_iter()
                .find(|entity| entity.id != world.entity)
                .map_or(Dynamic::UNIT, |entity| Dynamic::from(entity.id.as_str().to_owned()))
        })
        .register_fn("timer_fired", |world: &mut WorldView, key: &str| {
            world.snapshot.timers.get(&world.entity).is_some_and(|keys| keys.iter().any(|fired| fired == key))
        })
        .register_fn("contacts", |world: &mut WorldView| -> Array {
            world
                .snapshot
                .contacts
                .get(&world.entity)
                .map(|others| others.iter().map(|id| Dynamic::from(id.as_str().to_owned())).collect())
                .unwrap_or_default()
        })
        .register_fn("events", |world: &mut WorldView, name: &str| -> Array {
            world
                .snapshot
                .events
                .iter()
                .filter(|(event, _, _)| event == name)
                .map(|(_, sender, data)| {
                    let mut record = Map::new();
                    record.insert("sender".into(), Dynamic::from(sender.as_str().to_owned()));
                    record.insert("data".into(), to_script_value(name, data).unwrap_or(Dynamic::UNIT));
                    Dynamic::from_map(record)
                })
                .collect()
        })
        .register_fn("spawn", |world: &mut WorldView, definition: Map| {
            let value = from_script_value::<Value>("spawn", &Dynamic::from_map(definition)).unwrap_or(Value::Null);
            world.push(ScriptCommand::Spawn(value));
        })
        .register_fn("remove", |world: &mut WorldView, id: &str| world.push(ScriptCommand::Remove(id.to_owned())))
        .register_fn("send", |world: &mut WorldView, name: &str, data: Map| {
            let data = from_script_value::<Value>(name, &Dynamic::from_map(data)).unwrap_or(Value::Null);
            world.push(ScriptCommand::Send {
                name: name.to_owned(),
                data,
            });
        })
        .register_fn("start_timer", |world: &mut WorldView, key: &str, seconds: f64, repeat: bool| {
            world.push(ScriptCommand::StartTimer {
                key: key.to_owned(),
                seconds,
                repeat,
            });
        })
        .register_fn("set_velocity", |world: &mut WorldView, velocity: DVec3| {
            world.push(ScriptCommand::SetVelocity(velocity));
        })
        .register_fn("apply_impulse", |world: &mut WorldView, impulse: DVec3| {
            world.push(ScriptCommand::ApplyImpulse(impulse));
        })
        .register_fn("move_to", |world: &mut WorldView, position: DVec3| {
            world.push(ScriptCommand::MoveTo(position));
        });
}
