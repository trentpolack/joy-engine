// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A game's scripts as one library: every source by name, the hook scripts compiled from them, and the shared modules
//! they `require`. Hosts add every source up front (from `GameAssets`), compile the scripts whose hooks they call, read
//! data constants into Rust records, and hand edited sources back for hot reload.
//!
//! Imports name other sources in the library (`local parts = require("data/parts")`), never files, so scripts stay sandboxed.
//! A module's top level runs once when first imported and its export values are shared by every importer;
//! functions read them as `parts.NAME`, and a module's own functions read its constants as lexically captured locals.
//!
//! Hot reload follows imports: replacing a module recompiles it, every module that imports it, and every hook script
//! that depends on any of them, and commits only if all of them compile. A broken edit changes nothing. Hosts' script
//! states belong to their objects, so they survive reloads.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::rc::Rc;

use mlua::FromLua;
use mlua::Function;
use mlua::Lua;
use mlua::Table;
use mlua::Value;
use serde::de::DeserializeOwned;

use crate::IntoScriptArguments;
use crate::convert::from_script_value;
use crate::engine::ScriptEngine;
use crate::error::ScriptError;
use crate::script::Script;
use crate::script::ScriptHook;
use crate::script::ScriptState;

/// Every source in a library and the modules compiled from them; shared with the engine's import resolver.
#[derive(Clone, Debug, Default)]
struct ModuleTable {
    sources: BTreeMap<String, String>,
    modules: BTreeMap<String, Table>,
    // name → the scripts and modules that imported it, for reload invalidation.
    importers: BTreeMap<String, BTreeSet<String>>,
    // Modules being compiled right now, outermost first, for cycle detection.
    loading: Vec<String>,
}

/// Resolve an in-memory source. No borrow remains held while dependencies execute recursively.
fn resolve(lua: &Lua, table: &Rc<RefCell<ModuleTable>>, importer: &str, name: &str) -> mlua::Result<Table> {
    let source = {
        let mut modules = table.borrow_mut();
        modules.importers.entry(name.into()).or_default().insert(importer.into());
        if let Some(module) = modules.modules.get(name) {
            return Ok(module.clone());
        }
        if modules.loading.iter().any(|loading| loading == name) {
            return Err(mlua::Error::runtime(format!("require cycle: {} → {name}", modules.loading.join(" → "))));
        }
        let source = modules
            .sources
            .get(name)
            .cloned()
            .ok_or_else(|| mlua::Error::runtime(format!("no script named `{name}`")))?;
        modules.loading.push(name.into());
        source
    };
    let result: mlua::Result<Table> = (|| {
        crate::source::validate(&source)?;
        let environment = crate::engine::environment(lua)?;
        install_require(lua, &environment, name)?;
        let module = lua
            .load(&source)
            .set_mode(mlua::chunk::ChunkMode::Text)
            .set_name(name)
            .set_environment(environment)
            .eval::<Table>()?;
        crate::frozen::freeze(lua, &module)?;
        Ok(module)
    })();
    let mut modules = table.borrow_mut();
    modules.loading.pop();
    let module = result?;
    modules.modules.insert(name.into(), module.clone());
    Ok(module)
}

/// Bind each source's dependency name without exposing the internal resolver's importer argument to scripts.
pub(crate) fn install_require(lua: &Lua, environment: &Table, importer: &str) -> mlua::Result<()> {
    let resolver = lua.globals().raw_get::<Value>("__joy_require")?;
    if let Value::Function(resolver) = resolver {
        let importer = importer.to_string();
        environment.raw_set(
            "require",
            lua.create_function(move |_, name: String| resolver.call::<Table>((name, importer.clone())))?,
        )?;
    }
    Ok(())
}

/// A game's scripts: sources by name, compiled hook scripts, and imported modules, on one [`ScriptEngine`].
pub struct ScriptLibrary {
    engine: ScriptEngine,
    table: Rc<RefCell<ModuleTable>>,
    scripts: BTreeMap<String, Script>,
}

impl ScriptLibrary {
    /// A library on `engine` (with the host's bindings already registered). Installs the library's import resolver,
    /// replacing the engine's default of refusing every import.
    pub fn new(engine: ScriptEngine) -> Self {
        let table = Rc::new(RefCell::new(ModuleTable::default()));
        let resolver_table = Rc::clone(&table);
        let resolver = engine
            .lua()
            .create_function(move |lua, (name, importer): (String, String)| {
                resolve(lua, &resolver_table, &importer, &name)
            })
            .expect("the library resolver is a valid host function");
        engine.lua().globals().raw_set("__joy_require", resolver).expect("the engine owns its host globals");
        Self {
            engine,
            table,
            scripts: BTreeMap::new(),
        }
    }

    /// Add or replace a source without compiling anything. Names are the import spelling (`"data/parts"`); hosts
    /// usually derive them from asset paths. Use [`ScriptLibrary::replace_source`] to hot reload a source in use.
    pub fn add_source(&mut self, name: &str, source: &str) {
        self.table.borrow_mut().sources.insert(name.to_string(), source.to_string());
    }

    /// Compile the source `name` as a hook script the host will call, importing what it needs.
    pub fn compile_script(&mut self, name: &str, hooks: &[ScriptHook]) -> Result<(), ScriptError> {
        let Some(source) = self.source(name) else {
            return Err(ScriptError::UnknownScript {
                path: name.to_string(),
                name: name.to_string(),
            });
        };
        let script = Script::compile(&self.engine, name, &source, hooks)?;
        self.scripts.insert(name.to_string(), script);
        Ok(())
    }

    /// Compile one hook script, `name`, from several sources whose functions share a namespace (see
    /// [`Script::compile_parts`]), so rules split across files can call each other as methods. Reloading any part
    /// recompiles the script.
    pub fn compile_script_parts(
        &mut self,
        name: &str,
        parts: &[&str],
        hooks: &[ScriptHook],
    ) -> Result<(), ScriptError> {
        let sources = self.part_sources(name, parts.iter().map(|part| (*part).to_string()))?;
        let pairs: Vec<(&str, &str)> = parts.iter().copied().zip(sources.iter().map(String::as_str)).collect();
        let script = Script::compile_parts(&self.engine, name, &pairs, hooks)?;
        {
            let mut table = self.table.borrow_mut();
            for part in parts {
                if *part != name {
                    table.importers.entry((*part).to_string()).or_default().insert(name.to_string());
                }
            }
        }
        self.scripts.insert(name.to_string(), script);
        Ok(())
    }

    /// The engine, for limits and conversions.
    pub fn engine(&self) -> &ScriptEngine {
        &self.engine
    }

    /// A compiled hook script.
    pub fn script(&self, name: &str) -> Option<&Script> {
        self.scripts.get(name)
    }

    /// The current text of a source.
    pub fn source(&self, name: &str) -> Option<String> {
        self.table.borrow().sources.get(name).cloned()
    }

    /// Call `hook` on the compiled script `name` with `state` as `this` (see [`Script::call`]).
    pub fn call(
        &self,
        name: &str,
        hook: &str,
        state: &mut ScriptState,
        arguments: impl IntoScriptArguments,
    ) -> Result<Value, ScriptError> {
        self.compiled(name)?.call(&self.engine, hook, state, arguments)
    }

    /// [`ScriptLibrary::call`], reading the return value as `T` (see [`Script::call_as`]).
    pub fn call_as<T: crate::FromScriptValue>(
        &self,
        name: &str,
        hook: &str,
        state: &mut ScriptState,
        arguments: impl IntoScriptArguments,
    ) -> Result<T, ScriptError> {
        self.compiled(name)?.call_as(&self.engine, hook, state, arguments)
    }

    /// Read the export named `constant` from module `name` as a host record (compiling the module on first
    /// use). Hosts read authored data, such as a part catalog, this way; cache the result rather than calling per frame.
    pub fn constant<T: DeserializeOwned>(&self, name: &str, constant: &str) -> Result<T, ScriptError> {
        let module = self.module(name)?;
        let context = format!("{name} {constant}");
        let value = crate::frozen::backing(self.engine.lua(), &module)
            .and_then(|table| table.raw_get::<Value>(constant))
            .map_err(|error| ScriptError::Conversion {
                context: context.clone(),
                message: error.to_string(),
            })?;
        if value.is_nil() {
            return Err(ScriptError::Conversion {
                context,
                message: "no module export with this name".into(),
            });
        }
        from_script_value(self.engine.lua(), &context, &value)
    }

    /// Hot reload: replace source `name`, then recompile every module and hook script that depends on it. If any
    /// fails, nothing changes (the previous text keeps running) and the first error is returned. Returns the hook
    /// scripts that were recompiled.
    pub fn replace_source(&mut self, name: &str, source: &str) -> Result<Vec<String>, ScriptError> {
        let affected = self.dependents_of(name);
        let previous = self.table.borrow().clone();
        {
            let mut table = self.table.borrow_mut();
            table.sources.insert(name.into(), source.into());
            // Candidate chunks must never call an accepted module's closures: even a frozen export table
            // can contain functions with mutable lexical upvalues. Rebuild imports in a fresh module cache.
            // Persistent gameplay state belongs to ScriptState, not module initialization or upvalues.
            table.modules.clear();
            for importers in table.importers.values_mut() {
                importers.retain(|importer| !affected.contains(importer) && !previous.modules.contains_key(importer));
            }
        }

        let candidate = (|| {
            let recompiled = self.recompile(&affected)?;
            // Preserve lazy-cache membership and validate previously loaded modules even when no affected
            // hook imports them. All candidate evaluation finishes before accepting any script revision.
            for module in previous.modules.keys() {
                self.module(module)?;
            }
            Ok(recompiled)
        })();
        match candidate {
            Ok(recompiled) => {
                let names: Vec<String> = recompiled.keys().cloned().collect();
                self.scripts.extend(recompiled);
                Ok(names)
            }
            Err(error) => {
                *self.table.borrow_mut() = previous;
                Err(error)
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Implementation
    // -----------------------------------------------------------------------------------------------------------------

    fn compiled(&self, name: &str) -> Result<&Script, ScriptError> {
        self.scripts.get(name).ok_or_else(|| ScriptError::UnknownScript {
            path: name.to_string(),
            name: name.to_string(),
        })
    }

    /// The current text of each named part of script `script`.
    fn part_sources(&self, script: &str, parts: impl Iterator<Item = String>) -> Result<Vec<String>, ScriptError> {
        parts
            .map(|part| {
                self.source(&part).ok_or_else(|| ScriptError::UnknownScript {
                    path: script.to_string(),
                    name: part,
                })
            })
            .collect()
    }

    /// A compiled module, compiling it (and its imports) on first use.
    fn module(&self, name: &str) -> Result<Table, ScriptError> {
        let resolver = self
            .engine
            .lua()
            .globals()
            .raw_get::<Function>("__joy_require")
            .map_err(|error| ScriptError::from_compile(name, error))?;
        let value =
            self.engine.execute(resolver, (name, name)).map_err(|error| ScriptError::from_compile(name, error))?;
        Table::from_lua(value, self.engine.lua()).map_err(|error| ScriptError::from_compile(name, error))
    }

    /// `name` and everything that imports it, directly or through other modules.
    fn dependents_of(&self, name: &str) -> BTreeSet<String> {
        let table = self.table.borrow();
        let mut affected = BTreeSet::new();
        let mut pending = vec![name.to_string()];
        while let Some(next) = pending.pop() {
            if !affected.insert(next.clone()) {
                continue;
            }
            if let Some(importers) = table.importers.get(&next) {
                pending.extend(importers.iter().cloned());
            }
        }
        affected
    }

    /// Recompile the affected hook scripts against the updated sources, without committing them. Modules recompile
    /// lazily as these scripts import them; affected modules no script imports are compiled here so their errors
    /// surface now rather than at the next `constant` read.
    fn recompile(&self, affected: &BTreeSet<String>) -> Result<BTreeMap<String, Script>, ScriptError> {
        let mut recompiled = BTreeMap::new();
        for name in affected {
            let Some(script) = self.scripts.get(name) else {
                continue;
            };
            let sources = self.part_sources(name, script.parts().iter().cloned())?;
            recompiled.insert(name.clone(), script.recompiled(&self.engine, &sources)?);
            let mut table = self.table.borrow_mut();
            for part in script.parts() {
                if part != name {
                    table.importers.entry(part.clone()).or_default().insert(name.clone());
                }
            }
        }
        for name in affected {
            let part_of_a_script = self.scripts.values().any(|script| script.parts().iter().any(|part| part == name));
            if !self.scripts.contains_key(name) && !part_of_a_script {
                self.module(name)?;
            }
        }
        Ok(recompiled)
    }
}

impl std::fmt::Debug for ScriptLibrary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let table = self.table.borrow();
        formatter
            .debug_struct("ScriptLibrary")
            .field("sources", &table.sources.keys().collect::<Vec<_>>())
            .field("scripts", &self.scripts.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}
