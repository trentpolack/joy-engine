// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Compiled scripts, the hooks hosts call on them, and the per-object state those hooks keep.
//!
//! A script's top level runs once, at compile time, and may only declare what its functions share: `const` values,
//! read inside functions as `global::NAME`. Hooks are ordinary script functions; the host calls one with an object's
//! [`ScriptState`] bound as `this`, so `this.timer -= elapsed_seconds` persists between calls. Script parameters are
//! copies; helpers called as methods (`this.update_awareness(…)`) share the caller's `this`.
//!
//! Hot reload replaces a script's compiled program in place. States are separate values, so every object keeps its
//! fields through a reload; a source that fails to compile, or lacks a required hook, leaves the previous program
//! running.

use rhai::AST;
use rhai::CallFnOptions;
use rhai::Dynamic;
use rhai::FuncArgs;
use rhai::Map;
use rhai::Module;
use rhai::Scope;

use crate::engine::ScriptEngine;
use crate::error::ScriptError;

/// Call-stack name used for failures while a script's top level defines its constants.
const TOP_LEVEL: &str = "<top level>";

/// Prefix of the generated function that forwards a host call to each hook (see [`dispatcher_source`]).
pub(crate) const DISPATCH_PREFIX: &str = "__joy_hook_";

/// A function a host calls on a script, by name and parameter count (`this` is not a parameter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptHook {
    /// The script function's name.
    pub name: &'static str,
    /// Its parameter count.
    pub arity: usize,
}

impl ScriptHook {
    /// A hook named `name` taking `arity` parameters.
    pub const fn new(name: &'static str, arity: usize) -> Self {
        Self {
            name,
            arity,
        }
    }
}

/// One scripted object's state: the object map its hooks see as `this`. Owned by the host object, not the script, so
/// it survives hot reloads; dropping it releases everything the script stored.
#[derive(Clone, Debug)]
pub struct ScriptState {
    value: Dynamic,
}

impl ScriptState {
    /// An empty state; hooks (usually an `init` hook) add their fields.
    pub fn new() -> Self {
        Self {
            value: Dynamic::from_map(Map::new()),
        }
    }

    /// A field's value, or `None` when the script has not set it.
    pub fn get(&self, field: &str) -> Option<Dynamic> {
        self.value.read_lock::<Map>().and_then(|map| map.get(field).cloned())
    }

    /// A field's value as `T`, or `None` when it is unset or holds another type.
    pub fn get_as<T: Clone + 'static>(&self, field: &str) -> Option<T> {
        self.get(field).and_then(|value| value.try_cast::<T>())
    }

    /// Set a field, replacing any previous value.
    pub fn set(&mut self, field: &str, value: impl Clone + 'static) {
        if let Some(mut map) = self.value.write_lock::<Map>() {
            map.insert(field.into(), Dynamic::from(value));
        }
    }

    /// A state holding an existing object map (or any value a script returned for use as `this`).
    pub fn from_value(value: Dynamic) -> Self {
        Self {
            value,
        }
    }

    /// The state's value, for converting with [`crate::from_script_value`].
    pub fn value(&self) -> &Dynamic {
        &self.value
    }

    /// Read a field in place, without copying it out of the state; `None` when the field is unset. Hosts use this to
    /// read large script-owned collections (a battle's units) every step without cloning them.
    pub fn with_field<R>(&self, field: &str, read: impl FnOnce(&Dynamic) -> R) -> Option<R> {
        let map = self.value.read_lock::<Map>()?;
        map.get(field).map(read)
    }

    /// Change a field in place (a host edit to a script-owned record); does nothing when the field is unset.
    pub fn edit_field(&mut self, field: &str, edit: impl FnOnce(&mut Dynamic)) {
        if let Some(mut map) = self.value.write_lock::<Map>()
            && let Some(value) = map.get_mut(field)
        {
            edit(value);
        }
    }

    /// How many fields the state holds.
    pub fn len(&self) -> usize {
        self.value.read_lock::<Map>().map_or(0, |map| map.len())
    }

    /// Whether the state holds no fields.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ScriptState {
    fn default() -> Self {
        Self::new()
    }
}

/// A compiled script with the hooks its host requires. Compiled against one [`ScriptEngine`]; call it with the same
/// engine, whose registered host functions it uses.
#[derive(Clone, Debug)]
pub struct Script {
    path: String,
    // Source names whose functions make up the script; one, the path itself, unless compiled from parts.
    parts: Vec<String>,
    hooks: Vec<HookEntry>,
    program: AST,
    revision: u64,
}

/// A declared hook and the generated dispatcher the host calls for it.
#[derive(Clone, Debug)]
struct HookEntry {
    hook: ScriptHook,
    dispatcher: String,
}

impl Script {
    /// Compile `source`, run its top level once, and check that it defines every hook in `hooks`. `path` names the
    /// script in errors and `print`/`debug` output (a project-relative path such as `assets/scripts/ai.rhai`).
    pub fn compile(engine: &ScriptEngine, path: &str, source: &str, hooks: &[ScriptHook]) -> Result<Self, ScriptError> {
        Self::compile_parts(engine, path, &[(path, source)], hooks)
    }

    /// Compile one script from several sources (`(name, source)` pairs): each part runs its own top level, with its
    /// own constants and imports, and all their functions join one namespace, so a function in one part can call
    /// another part's functions directly or as methods (`this.units[i].advance(...)`), which `import` does not allow.
    /// `path` names the script; errors name the part that failed. Hooks may live in any part.
    pub fn compile_parts(
        engine: &ScriptEngine,
        path: &str,
        parts: &[(&str, &str)],
        hooks: &[ScriptHook],
    ) -> Result<Self, ScriptError> {
        let hooks: Vec<HookEntry> = hooks
            .iter()
            .map(|hook| HookEntry {
                hook: *hook,
                dispatcher: format!("{DISPATCH_PREFIX}{}", hook.name),
            })
            .collect();
        let program = prepare(engine, path, parts, &hooks)?;
        Ok(Self {
            path: path.to_string(),
            parts: parts.iter().map(|(name, _)| (*name).to_string()).collect(),
            hooks,
            program,
            revision: 0,
        })
    }

    /// Hot reload: compile `source` with the same path and hooks and, if that succeeds, replace the program and
    /// advance [`Script::revision`]. On failure the previous program keeps running and the error is returned.
    pub fn replace_source(&mut self, engine: &ScriptEngine, source: &str) -> Result<(), ScriptError> {
        self.program = prepare(engine, &self.path, &[(self.path.as_str(), source)], &self.hooks)?;
        self.revision += 1;
        Ok(())
    }

    /// This script recompiled from its parts' current `sources` (in [`Script::parts`] order) with the same path and
    /// hooks, one revision later; `self` is unchanged. [`crate::ScriptLibrary`] uses it to recompile a script and its
    /// dependents before committing any of them.
    pub(crate) fn recompiled(&self, engine: &ScriptEngine, sources: &[String]) -> Result<Self, ScriptError> {
        let parts: Vec<(&str, &str)> =
            self.parts.iter().map(String::as_str).zip(sources.iter().map(String::as_str)).collect();
        Ok(Self {
            path: self.path.clone(),
            parts: self.parts.clone(),
            hooks: self.hooks.clone(),
            program: prepare(engine, &self.path, &parts, &self.hooks)?,
            revision: self.revision + 1,
        })
    }

    /// The source names the script was compiled from.
    pub fn parts(&self) -> &[String] {
        &self.parts
    }

    /// The script's path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The hooks every revision must define.
    pub fn hooks(&self) -> impl Iterator<Item = ScriptHook> + '_ {
        self.hooks.iter().map(|entry| entry.hook)
    }

    /// Zero when compiled, plus one per accepted [`Script::replace_source`].
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Call `hook` with `state` bound as `this` and `arguments` as its parameters (a tuple; values are copied into the
    /// script). Returns what the hook returned, or `()` if it returned nothing. Only hooks declared at compile time can
    /// be called. A failure, including an exceeded limit, ends only this call; changes the hook made to `state` before
    /// failing are kept.
    pub fn call(
        &self,
        engine: &ScriptEngine,
        hook: &str,
        state: &mut ScriptState,
        arguments: impl FuncArgs,
    ) -> Result<Dynamic, ScriptError> {
        let Some(entry) = self.hooks.iter().find(|entry| entry.hook.name == hook) else {
            return Err(ScriptError::UndeclaredHook {
                path: self.path.clone(),
                name: hook.to_string(),
            });
        };

        // Constants were captured when the top level ran at compile time; never rerun it per call.
        let options = CallFnOptions::new().eval_ast(false).rewind_scope(true).bind_this_ptr(&mut state.value);
        let mut scope = Scope::new();
        engine
            .rhai()
            .call_fn_with_options::<Dynamic>(options, &mut scope, &self.program, &entry.dispatcher, arguments)
            .map_err(|error| ScriptError::from_runtime(&self.path, hook, engine.limits().operations_per_call, *error))
    }

    /// [`Script::call`], reading the hook's return value as `T`; a [`ScriptError::ReturnType`] for any other type.
    pub fn call_as<T: Clone + 'static>(
        &self,
        engine: &ScriptEngine,
        hook: &str,
        state: &mut ScriptState,
        arguments: impl FuncArgs,
    ) -> Result<T, ScriptError> {
        let value = self.call(engine, hook, state, arguments)?;
        let found = value.type_name().to_string();
        value.try_cast::<T>().ok_or_else(|| ScriptError::ReturnType {
            path: self.path.clone(),
            hook: hook.to_string(),
            expected: engine.rhai().map_type_name(std::any::type_name::<T>()).to_string(),
            found: engine.rhai().map_type_name(&found).to_string(),
        })
    }
}

/// Parse each part, run its top level into an encapsulated module (its constants become its functions' `global::`
/// namespace), merge the parts' functions, check the required hooks, and add their dispatchers.
fn prepare(engine: &ScriptEngine, path: &str, parts: &[(&str, &str)], hooks: &[HookEntry]) -> Result<AST, ScriptError> {
    let mut program = AST::empty();
    for (name, source) in parts {
        program = program.merge(&prepare_part(engine, name, source)?);
    }
    program.set_source(path);

    for entry in hooks {
        let hook = entry.hook;
        let defined =
            program.iter_functions().any(|function| function.name == hook.name && function.params.len() == hook.arity);
        if !defined {
            return Err(ScriptError::MissingHook {
                path: path.to_string(),
                name: hook.name.to_string(),
                arity: hook.arity,
            });
        }
    }

    let dispatchers: String = hooks.iter().map(dispatcher_source).collect();
    let dispatchers = engine.rhai().compile(&dispatchers).map_err(|error| ScriptError::from_parse(path, error))?;
    Ok(program.merge(&dispatchers))
}

/// One part's functions, with its top level run and its constants captured.
fn prepare_part(engine: &ScriptEngine, name: &str, source: &str) -> Result<AST, ScriptError> {
    let mut parsed = engine.rhai().compile(source).map_err(|error| ScriptError::from_parse(name, error))?;
    parsed.set_source(name);

    let module =
        Module::eval_ast_as_new(Scope::new(), &parsed, engine.rhai()).map_err(
            |error| match ScriptError::from_runtime(name, TOP_LEVEL, engine.limits().operations_per_call, *error) {
                ScriptError::Runtime {
                    location,
                    message,
                    ..
                } => ScriptError::Compile {
                    location,
                    message,
                },
                other => other,
            },
        )?;
    let mut program = AST::new_from_module(module);
    program.set_source(name);
    Ok(program)
}

/// `fn __joy_hook_tick(argument_0) { this.tick(argument_0) }`: a forwarding function for one hook.
///
/// Rhai runs a function called directly from Rust without the environment its module captured, so a hook reading
/// `global::NAME` would fail; a function called from script keeps it. Each host call therefore enters through a
/// generated dispatcher that calls the hook as a method, sharing `this`.
fn dispatcher_source(entry: &HookEntry) -> String {
    let parameters: Vec<String> = (0..entry.hook.arity).map(|index| format!("argument_{index}")).collect();
    let parameters = parameters.join(", ");
    format!("fn {}({parameters}) {{ this.{}({parameters}) }}\n", entry.dispatcher, entry.hook.name)
}
