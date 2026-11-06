// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A game's scripts as one library: every source by name, the hook scripts compiled from them, and the shared modules
//! they `import`. Hosts add every source up front (from `GameAssets`), compile the scripts whose hooks they call, read
//! data constants into Rust records, and hand edited sources back for hot reload.
//!
//! Imports name other sources in the library (`import "data/parts" as parts;`), never files, so scripts stay sandboxed.
//! A module's top level runs once when first imported and its `export const` values are shared by every importer;
//! functions read them as `parts::NAME`, and a module's own functions read its constants as `global::NAME`.
//!
//! Hot reload follows imports: replacing a module recompiles it, every module that imports it, and every hook script
//! that depends on any of them, and commits only if all of them compile. A broken edit changes nothing. Hosts' script
//! states belong to their objects, so they survive reloads.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::rc::Rc;

use rhai::Dynamic;
use rhai::Engine;
use rhai::EvalAltResult;
use rhai::FuncArgs;
use rhai::Module;
use rhai::ModuleResolver;
use rhai::Position;
use rhai::Scope;
use serde::de::DeserializeOwned;

use crate::convert::from_script_value;
use crate::engine::ScriptEngine;
use crate::error::ScriptError;
use crate::script::Script;
use crate::script::ScriptHook;
use crate::script::ScriptState;

/// Every source in a library and the modules compiled from them; shared with the engine's import resolver.
#[derive(Debug, Default)]
struct ModuleTable {
    sources: BTreeMap<String, String>,
    modules: BTreeMap<String, Rc<Module>>,
    // name → the scripts and modules that imported it, for reload invalidation.
    importers: BTreeMap<String, BTreeSet<String>>,
    // Modules being compiled right now, outermost first, for cycle detection.
    loading: Vec<String>,
}

/// The engine's resolver for `import`: compiles library sources on first use and records who imported them.
struct LibraryResolver {
    table: Rc<RefCell<ModuleTable>>,
}

impl ModuleResolver for LibraryResolver {
    fn resolve(
        &self,
        engine: &Engine,
        importer: Option<&str>,
        name: &str,
        position: Position,
    ) -> Result<Rc<Module>, Box<EvalAltResult>> {
        // Record the edge first so a failed import still reloads its importer once the module is fixed.
        let source = {
            let mut table = self.table.borrow_mut();
            if let Some(importer) = importer {
                table.importers.entry(name.to_string()).or_default().insert(importer.to_string());
            }
            if let Some(module) = table.modules.get(name) {
                return Ok(Rc::clone(module));
            }
            if table.loading.iter().any(|loading| loading == name) {
                let cycle = format!("import cycle: {} → {name}", table.loading.join(" → "));
                return Err(Box::new(EvalAltResult::ErrorRuntime(cycle.into(), position)));
            }
            let Some(source) = table.sources.get(name).cloned() else {
                return Err(Box::new(EvalAltResult::ErrorModuleNotFound(name.to_string(), position)));
            };
            table.loading.push(name.to_string());
            source
        };

        // No borrow is held while the module runs: its own imports re-enter this resolver.
        let module = compile_module(engine, name, &source);
        let mut table = self.table.borrow_mut();
        table.loading.retain(|loading| loading != name);
        let module = Rc::new(module?);
        table.modules.insert(name.to_string(), Rc::clone(&module));
        Ok(module)
    }
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
    pub fn new(mut engine: ScriptEngine) -> Self {
        let table = Rc::new(RefCell::new(ModuleTable::default()));
        engine.rhai_mut().set_module_resolver(LibraryResolver {
            table: Rc::clone(&table),
        });
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
        arguments: impl FuncArgs,
    ) -> Result<Dynamic, ScriptError> {
        self.compiled(name)?.call(&self.engine, hook, state, arguments)
    }

    /// [`ScriptLibrary::call`], reading the return value as `T` (see [`Script::call_as`]).
    pub fn call_as<T: Clone + 'static>(
        &self,
        name: &str,
        hook: &str,
        state: &mut ScriptState,
        arguments: impl FuncArgs,
    ) -> Result<T, ScriptError> {
        self.compiled(name)?.call_as(&self.engine, hook, state, arguments)
    }

    /// Read the `export const` named `constant` from module `name` as a host record (compiling the module on first
    /// use). Hosts read authored data, such as a part catalog, this way; cache the result rather than calling per frame.
    pub fn constant<T: DeserializeOwned>(&self, name: &str, constant: &str) -> Result<T, ScriptError> {
        let module = self.module(name)?;
        let context = format!("{name} {constant}");
        let Some(value) = module.get_var_value::<Dynamic>(constant) else {
            return Err(ScriptError::Conversion {
                context,
                message: "no `export const` with this name".to_string(),
            });
        };
        from_script_value(&context, &value)
    }

    /// Hot reload: replace source `name`, then recompile every module and hook script that depends on it. If any
    /// fails, nothing changes (the previous text keeps running) and the first error is returned. Returns the hook
    /// scripts that were recompiled.
    pub fn replace_source(&mut self, name: &str, source: &str) -> Result<Vec<String>, ScriptError> {
        let affected = self.dependents_of(name);
        let previous_source = self.source(name);
        let previous_modules = {
            let mut table = self.table.borrow_mut();
            let previous = table.modules.clone();
            table.sources.insert(name.to_string(), source.to_string());
            for module in &affected {
                table.modules.remove(module);
            }
            previous
        };

        match self.recompile(&affected) {
            Ok(recompiled) => {
                let names: Vec<String> = recompiled.keys().cloned().collect();
                self.scripts.extend(recompiled);
                Ok(names)
            }
            Err(error) => {
                let mut table = self.table.borrow_mut();
                match previous_source {
                    Some(text) => table.sources.insert(name.to_string(), text),
                    None => table.sources.remove(name),
                };
                table.modules = previous_modules;
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
    fn module(&self, name: &str) -> Result<Rc<Module>, ScriptError> {
        if let Some(module) = self.table.borrow().modules.get(name) {
            return Ok(Rc::clone(module));
        }
        let resolver = LibraryResolver {
            table: Rc::clone(&self.table),
        };
        resolver.resolve(self.engine.rhai(), None, name, Position::NONE).map_err(|error| {
            ScriptError::from_runtime(name, "<top level>", self.engine.limits().operations_per_call, *error)
        })
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

/// Compile a module source and run its top level into a shared module.
fn compile_module(engine: &Engine, name: &str, source: &str) -> Result<Module, Box<EvalAltResult>> {
    let mut parsed = engine.compile(source).map_err(|error| Box::new(EvalAltResult::from(error)))?;
    parsed.set_source(name);
    Module::eval_ast_as_new(Scope::new(), &parsed, engine)
}
