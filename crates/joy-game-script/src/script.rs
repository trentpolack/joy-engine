// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Compiled hook functions and independently owned persistent state.

use mlua::Function;
use mlua::Lua;
use mlua::Table;
use mlua::Value;

use crate::ScriptEngine;
use crate::ScriptError;
use crate::engine::ensure_owner;
use crate::{
    IntoScriptArguments,
    IntoScriptValue,
};

/// A host-called function. Its Lua signature begins with explicit `this`, followed by `arity` arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptHook {
    /// Function name in the script's private environment.
    pub name: &'static str,
    /// Number of host arguments, excluding the leading `this` parameter.
    pub arity: usize,
}
impl ScriptHook {
    /// Declare one required hook.
    pub const fn new(name: &'static str, arity: usize) -> Self {
        Self {
            name,
            arity,
        }
    }
}

/// One object's state, allocated lazily on its first call or [`Self::set`]. Once bound it belongs to that VM.
/// Cloning shares its table, matching Lua reference semantics. States and their VM remain alive while held by Rust.
#[derive(Clone, Debug)]
pub struct ScriptState {
    value: Value,
    lua: Option<Lua>,
}
impl ScriptState {
    /// An empty state that has not yet been attached to a VM.
    pub fn new() -> Self {
        Self {
            value: Value::Nil,
            lua: None,
        }
    }
    /// A field handle, or `None` for an unset field. Tables and userdata retain reference identity.
    pub fn get(&self, field: &str) -> Option<Value> {
        let Value::Table(table) = &self.value else {
            return None;
        };
        let value = table.raw_get::<Value>(field).ok()?;
        (!value.is_nil()).then_some(value)
    }
    /// A field as `T`, or `None` if missing or not convertible to `T`.
    pub fn get_as<T: crate::FromScriptValue>(&self, field: &str) -> Option<T> {
        crate::results::value(self.lua.as_ref()?, self.get(field)?).ok()
    }
    /// Set a field, binding a new state to `lua` if needed. Values from another VM are rejected.
    pub fn set(&mut self, lua: &Lua, field: &str, value: impl IntoScriptValue) -> mlua::Result<()> {
        let value = crate::arguments::value(lua, value)?;
        ensure_owner(lua, &value)?;
        self.bind(lua)?.raw_set(field, value)
    }
    /// Adopt an existing table as object state. Other value types and expired VM handles are errors.
    pub fn from_value(value: Value) -> mlua::Result<Self> {
        let Value::Table(table) = &value else {
            return Err(mlua::Error::runtime("script state must be a table"));
        };
        let lua = table.weak_lua().try_upgrade().ok_or_else(|| mlua::Error::runtime("script state VM has expired"))?;
        Ok(Self {
            value,
            lua: Some(lua),
        })
    }
    /// The table value, or `Nil` before the first binding, for [`crate::from_script_value`].
    pub fn value(&self) -> &Value {
        &self.value
    }
    /// Read a field handle without copying the collection it references.
    pub fn with_field<R>(&self, field: &str, read: impl FnOnce(&Value) -> R) -> Option<R> {
        self.get(field).as_ref().map(read)
    }
    /// Edit an existing field and write its replacement back. An absent field is unchanged.
    pub fn edit_field(&mut self, field: &str, edit: impl FnOnce(&mut Value)) -> mlua::Result<()> {
        if let Some(mut value) = self.get(field) {
            edit(&mut value);
            let lua = self.lua.as_ref().expect("a table state owns its VM").clone();
            self.set(&lua, field, value)?;
        }
        Ok(())
    }
    /// Number of fields in the state table.
    pub fn len(&self) -> usize {
        match &self.value {
            Value::Table(table) => table.pairs::<Value, Value>().count(),
            _ => 0,
        }
    }
    /// Whether the table has no fields.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub(crate) fn bind(&mut self, lua: &Lua) -> mlua::Result<Table> {
        ensure_owner(lua, &self.value)?;
        if let Value::Table(table) = &self.value {
            return Ok(table.clone());
        }
        let table = lua.create_table()?;
        self.value = Value::Table(table.clone());
        self.lua = Some(lua.clone());
        Ok(table)
    }
}
impl Default for ScriptState {
    fn default() -> Self {
        Self::new()
    }
}

/// Hook functions compiled together in a private environment. The originating VM must also be used for calls.
#[derive(Clone, Debug)]
pub struct Script {
    path: String,
    parts: Vec<String>,
    hooks: Vec<ScriptHook>,
    environment: Table,
    revision: u64,
    // Keep the VM alive after its function handles and environment are released.
    lua: Lua,
}
impl Script {
    /// Compile source, evaluate its declarations once, and validate every required hook, including its arity.
    pub fn compile(engine: &ScriptEngine, path: &str, source: &str, hooks: &[ScriptHook]) -> Result<Self, ScriptError> {
        Self::compile_parts(engine, path, &[(path, source)], hooks)
    }
    /// Compile sources into one private global namespace. Global function declarations are shared across parts;
    /// each part's locals stay lexical to that part. Hooks take explicit `this` as their first parameter.
    pub fn compile_parts(
        engine: &ScriptEngine,
        path: &str,
        parts: &[(&str, &str)],
        hooks: &[ScriptHook],
    ) -> Result<Self, ScriptError> {
        let environment = prepare(engine, path, parts, hooks)?;
        Ok(Self {
            path: path.into(),
            parts: parts.iter().map(|(name, _)| (*name).into()).collect(),
            hooks: hooks.to_vec(),
            environment,
            revision: 0,
            lua: engine.lua().clone(),
        })
    }
    /// Replace this script only after the new source and all required hooks validate. Object states are untouched.
    pub fn replace_source(&mut self, engine: &ScriptEngine, source: &str) -> Result<(), ScriptError> {
        self.check_engine(engine)?;
        self.environment = prepare(engine, &self.path, &[(self.path.as_str(), source)], &self.hooks)?;
        self.revision += 1;
        Ok(())
    }
    pub(crate) fn recompiled(&self, engine: &ScriptEngine, sources: &[String]) -> Result<Self, ScriptError> {
        let parts: Vec<_> = self.parts.iter().map(String::as_str).zip(sources.iter().map(String::as_str)).collect();
        let mut next = Self::compile_parts(engine, &self.path, &parts, &self.hooks)?;
        next.revision = self.revision + 1;
        Ok(next)
    }
    /// Source names used to compile this script, in evaluation order.
    pub fn parts(&self) -> &[String] {
        &self.parts
    }
    /// Diagnostic path supplied by the host.
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Hooks required by every accepted revision.
    pub fn hooks(&self) -> impl Iterator<Item = ScriptHook> + '_ {
        self.hooks.iter().copied()
    }
    /// Number of accepted replacements since initial compilation.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Call a declared hook. Tables and userdata are references, scalars are values; state mutations before an
    /// error are preserved. Arguments and state must belong to this engine's VM. No return value becomes `Nil`.
    pub fn call(
        &self,
        engine: &ScriptEngine,
        hook: &str,
        state: &mut ScriptState,
        arguments: impl IntoScriptArguments,
    ) -> Result<Value, ScriptError> {
        self.check_engine(engine)?;
        let Some(entry) = self.hooks.iter().find(|entry| entry.name == hook) else {
            return Err(ScriptError::UndeclaredHook {
                path: self.path.clone(),
                name: hook.into(),
            });
        };
        let call = || -> mlua::Result<Value> {
            let mut arguments = arguments.into_script_arguments(engine.lua())?;
            if arguments.len() != entry.arity {
                return Err(mlua::Error::runtime(format!(
                    "hook `{hook}` expects {} host arguments, received {}",
                    entry.arity,
                    arguments.len()
                )));
            }
            for argument in &arguments {
                ensure_owner(engine.lua(), argument)?;
            }
            arguments.push_front(Value::Table(state.bind(engine.lua())?));
            let function = self.environment.raw_get::<Function>(hook)?;
            engine.execute(function, arguments)
        };
        call().map_err(|error| ScriptError::from_runtime(&self.path, hook, error))
    }
    /// Call a hook and read its result as `T`, reporting a typed return-value mismatch.
    pub fn call_as<T: crate::FromScriptValue>(
        &self,
        engine: &ScriptEngine,
        hook: &str,
        state: &mut ScriptState,
        arguments: impl IntoScriptArguments,
    ) -> Result<T, ScriptError> {
        let value = self.call(engine, hook, state, arguments)?;
        let found = value.type_name().to_string();
        let mismatch = || ScriptError::ReturnType {
            path: self.path.clone(),
            hook: hook.into(),
            expected: std::any::type_name::<T>().into(),
            found: found.clone(),
        };
        crate::results::value(engine.lua(), value).map_err(|_| mismatch())
    }

    fn check_engine(&self, engine: &ScriptEngine) -> Result<(), ScriptError> {
        if self.lua.weak() != engine.lua().weak() {
            return Err(ScriptError::Conversion {
                context: self.path.clone(),
                message: "script belongs to another Lua VM".into(),
            });
        }
        Ok(())
    }
}

fn prepare(
    engine: &ScriptEngine,
    path: &str,
    parts: &[(&str, &str)],
    hooks: &[ScriptHook],
) -> Result<Table, ScriptError> {
    let environment = engine.environment().map_err(|error| ScriptError::from_compile(path, error))?;
    for (name, source) in parts {
        crate::library::install_require(engine.lua(), &environment, name)
            .map_err(|error| ScriptError::from_compile(name, error))?;
        crate::source::validate(source).map_err(|error| ScriptError::from_compile(name, error))?;
        let function = engine
            .lua()
            .load(*source)
            .set_mode(mlua::chunk::ChunkMode::Text)
            .set_name(*name)
            .set_environment(environment.clone())
            .into_function()
            .map_err(|error| ScriptError::from_compile(name, error))?;
        engine.execute(function, ()).map_err(|error| ScriptError::from_compile(name, error))?;
    }
    for hook in hooks {
        let valid = environment.raw_get::<Function>(hook.name).is_ok_and(|function| {
            let info = function.info();
            info.what == "Lua" && usize::from(info.num_params) == hook.arity + 1 && !info.is_vararg
        });
        if !valid {
            return Err(ScriptError::MissingHook {
                path: path.into(),
                name: hook.name.into(),
                arity: hook.arity,
            });
        }
    }
    Ok(environment)
}
