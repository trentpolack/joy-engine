// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scripted entities: an authored `script` component names a game script whose hooks drive the entity, run by a
//! [`ScriptSystem`] against a read-only view of the world. Scripts never mutate simulation state directly: they return
//! commands (spawn, remove, send an event, start a timer, push a body) that the system applies after each hook, so
//! Rust owns every structural change and the update order stays the world's.
//!
//! Every entity script defines [`ENTITY_SCRIPT_HOOKS`]:
//!
//! ```lua
//! function init(this, entity)
//!     this.speed = entity:component("script").speed
//! end
//!
//! function update(this, entity, world, elapsed_seconds)
//!     local target = world:nearest(entity.position, 12.0, "player")
//!     if target ~= nil then
//!         local heading = world:position(target) - entity.position
//!         world:set_velocity(heading / heading:length() * this.speed)
//!     end
//!     if world:timer_fired("attack") then world:send("attacked", { target = target }) end
//! end
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
use joy_game_script::mlua::{
    Error,
    Table,
    UserData,
    UserDataFields,
    UserDataMethods,
    Value as LuaValue,
};
use joy_game_script::{
    ScriptEngine,
    ScriptHook,
    ScriptLibrary,
    ScriptLimits,
    ScriptState,
    ScriptUserData,
    ScriptVec3,
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

/// Hooks every entity script defines: `init(this, entity)` once before its first update, and
/// `update(this, entity, world, elapsed_seconds)` every world update.
pub const ENTITY_SCRIPT_HOOKS: [ScriptHook; 2] = [ScriptHook::new(INIT_HOOK, 1), ScriptHook::new(UPDATE_HOOK, 3)];

/// Field of the `script` component naming the library script.
pub const SCRIPT_NAME_FIELD: &str = "script";

const INIT_HOOK: &str = "init";
const UPDATE_HOOK: &str = "update";

/// Most distinct failure messages remembered, so a script failing differently every update cannot grow memory.
const MAXIMUM_REPORTED_FAILURES: usize = 64;

// ---------------------------------------------------------------------------------------------------------------------
// Public Records
// ---------------------------------------------------------------------------------------------------------------------

/// An event a script sent with `world:send(name, data)`; scripts read them with `world:events(name)` during the next
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
    ScriptEngine::new(limits)
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
            if let Err(error) = library.call(&script, INIT_HOOK, &mut state, (ScriptUserData(view.clone()),)) {
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
        let result = library.call(
            &script,
            UPDATE_HOOK,
            state,
            (ScriptUserData(view.clone()), ScriptUserData(world), f64::from(elapsed_seconds)),
        );
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

impl UserData for EntityView {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, entity| Ok(entity.id.as_str().to_owned()));
        fields.add_field_method_get("kind", |_, entity| Ok(entity.kind.clone()));
        fields.add_field_method_get("position", |_, entity| Ok(ScriptVec3(entity.position)));
        fields.add_field_method_get("velocity", |_, entity| Ok(ScriptVec3(entity.velocity)));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods
            .add_method("has_tag", |_, entity, tag: String| Ok(entity.tags.iter().any(|candidate| candidate == &tag)));
        methods.add_method("component", |lua, entity, kind: String| match entity.components.get(&kind) {
            Some(record) => to_script_value(lua, &kind, record).map_err(Error::external),
            None => Ok(LuaValue::Nil),
        });
    }
}

impl UserData for WorldView {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("position", |_, world, id: String| {
            Ok(world
                .snapshot
                .index
                .get(id.as_str())
                .and_then(|index| world.snapshot.entities.get(*index))
                .map(|entity| ScriptVec3(entity.position)))
        });
        methods.add_method("kind", |_, world, id: String| {
            Ok(world
                .snapshot
                .index
                .get(id.as_str())
                .and_then(|index| world.snapshot.entities.get(*index))
                .map(|entity| entity.kind.clone()))
        });
        methods.add_method("within", |_, world, (center, radius, tag): (ScriptVec3, f64, String)| {
            Ok(world
                .snapshot
                .within(center.0, radius, &tag)
                .into_iter()
                .filter(|entity| entity.id != world.entity)
                .map(|entity| entity.id.as_str().to_owned())
                .collect::<Vec<_>>())
        });
        methods.add_method("nearest", |_, world, (center, radius, tag): (ScriptVec3, f64, String)| {
            Ok(world
                .snapshot
                .within(center.0, radius, &tag)
                .into_iter()
                .find(|entity| entity.id != world.entity)
                .map(|entity| entity.id.as_str().to_owned()))
        });
        methods.add_method("timer_fired", |_, world, key: String| {
            Ok(world.snapshot.timers.get(&world.entity).is_some_and(|keys| keys.iter().any(|fired| fired == &key)))
        });
        methods.add_method("contacts", |_, world, ()| {
            Ok(world
                .snapshot
                .contacts
                .get(&world.entity)
                .map(|others| others.iter().map(|id| id.as_str().to_owned()).collect::<Vec<_>>())
                .unwrap_or_default())
        });
        methods.add_method("events", |lua, world, name: String| {
            world
                .snapshot
                .events
                .iter()
                .filter(|(event, _, _)| event == &name)
                .map(|(_, sender, data)| {
                    let record = lua.create_table()?;
                    record.set("sender", sender.as_str())?;
                    record.set("data", to_script_value(lua, &name, data).map_err(Error::external)?)?;
                    Ok(record)
                })
                .collect::<Result<Vec<_>, Error>>()
        });
        methods.add_method("spawn", |lua, world, definition: Table| {
            let value =
                from_script_value::<Value>(lua, "spawn", &LuaValue::Table(definition)).map_err(Error::external)?;
            world.push(ScriptCommand::Spawn(value));
            Ok(())
        });
        methods.add_method("remove", |_, world, id: String| {
            world.push(ScriptCommand::Remove(id));
            Ok(())
        });
        methods.add_method("send", |lua, world, (name, data): (String, Table)| {
            let data = from_script_value::<Value>(lua, &name, &LuaValue::Table(data)).map_err(Error::external)?;
            world.push(ScriptCommand::Send {
                name,
                data,
            });
            Ok(())
        });
        methods.add_method("start_timer", |_, world, (key, seconds, repeat): (String, f64, bool)| {
            world.push(ScriptCommand::StartTimer {
                key,
                seconds,
                repeat,
            });
            Ok(())
        });
        methods.add_method("set_velocity", |_, world, velocity: ScriptVec3| {
            world.push(ScriptCommand::SetVelocity(velocity.0));
            Ok(())
        });
        methods.add_method("apply_impulse", |_, world, impulse: ScriptVec3| {
            world.push(ScriptCommand::ApplyImpulse(impulse.0));
            Ok(())
        });
        methods.add_method("move_to", |_, world, position: ScriptVec3| {
            world.push(ScriptCommand::MoveTo(position.0));
            Ok(())
        });
    }
}
