// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A private Lua VM with deterministic bindings and bounded calls.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use mlua::Function;
use mlua::HookTriggers;
use mlua::IntoLuaMulti;
use mlua::Lua;
use mlua::LuaOptions;
use mlua::StdLib;
use mlua::Table;
use mlua::Value;
use mlua::VmState;

use crate::IntoScriptValue;
use crate::standard_library;

/// Lua VM instructions permitted per call, including nested imports and protected calls.
/// Native library work and trusted host callbacks must bound their own execution time.
pub const DEFAULT_INSTRUCTIONS_PER_CALL: u64 = 2_000_000;
/// Total Lua allocation ceiling per engine, including all scripts, modules, and persistent object states.
pub const DEFAULT_MEMORY_BYTES: usize = 64 * 1024 * 1024;

/// Lua execution and allocation limits. A failed call preserves mutations already made to object state.
/// Lua enforces its own fixed parser and stack-depth limits; those limits are not configurable here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptLimits {
    /// VM instructions per top-level evaluation or hook call; zero disables the instruction budget.
    pub instructions_per_call: u64,
    /// Total bytes allocated by this VM; zero disables the memory ceiling. Host Rust allocations are excluded.
    pub memory_bytes: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        Self {
            instructions_per_call: DEFAULT_INSTRUCTIONS_PER_CALL,
            memory_bytes: DEFAULT_MEMORY_BYTES,
        }
    }
}

/// One engine owns one Lua VM. Register host bindings through [`Self::lua`] before compiling scripts.
/// Scripts execute in independent environments and cannot access files, clocks, dynamic source loaders, or coroutines.
pub struct ScriptEngine {
    lua: Lua,
    limits: ScriptLimits,
    budget: Rc<ExecutionBudget>,
}

#[derive(Default)]
struct ExecutionBudget {
    remaining: Cell<u64>,
    active: Cell<bool>,
    exhausted: Cell<bool>,
}

impl ScriptEngine {
    /// Construct the sandbox and portable standard library. Invalid authored programs fail when compiled or called.
    /// `memory_bytes` is applied after the VM's fixed standard-library setup.
    pub fn new(limits: ScriptLimits) -> Self {
        let lua = Lua::new_with(StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8, LuaOptions::default())
            .expect("the selected Lua standard libraries are safe");
        crate::frozen::register(&lua).expect("readonly table support is valid");
        standard_library::register(&lua).expect("engine standard library registration succeeds");
        let budget = Rc::new(ExecutionBudget::default());
        let observed = Rc::clone(&budget);
        lua.set_global_hook(HookTriggers::new().every_nth_instruction(1), move |_, _| {
            if !observed.active.get() {
                return Ok(VmState::Continue);
            }
            let remaining = observed.remaining.get();
            if remaining == 0 {
                observed.exhausted.set(true);
                return Err(mlua::Error::runtime("exceeded the instruction budget"));
            }
            observed.remaining.set(remaining - 1);
            Ok(VmState::Continue)
        })
        .expect("Lua supports count hooks");
        let protected_budget = Rc::clone(&budget);
        let check_budget = lua
            .create_function(move |_, ()| {
                if protected_budget.exhausted.get() {
                    return Err(mlua::Error::runtime("exceeded the instruction budget"));
                }
                Ok(())
            })
            .expect("the budget guard is a valid host callback");
        lua.load(include_str!("protected_calls.lua"))
            .set_name("joy/protected_calls.lua")
            .call::<()>(check_budget)
            .expect("protected calls preserve the execution budget");
        lua.set_memory_limit(limits.memory_bytes).expect("Lua supports allocation ceilings");
        Self {
            lua,
            limits,
            budget,
        }
    }

    /// The limits subsequent script calls use.
    pub fn limits(&self) -> ScriptLimits {
        self.limits
    }

    /// Replace limits for subsequent calls. Lowering memory below current usage rejects further allocations
    /// until garbage collection or state cleanup brings usage below the new ceiling.
    pub fn set_limits(&mut self, limits: ScriptLimits) {
        self.lua.set_memory_limit(limits.memory_bytes).expect("Lua supports allocation ceilings");
        self.limits = limits;
    }

    /// Host access to this VM for binding userdata, globals, and serde values. Host code is trusted: executing
    /// source directly through this handle bypasses the engine's hook budget and private environments.
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Expose named host values as a readonly enumeration table (`TypeName.Variant`). Register userdata's
    /// equality and string conversion through its `UserData` implementation when using userdata variants.
    pub fn register_enum<T: Clone + IntoScriptValue>(
        &mut self,
        type_name: &str,
        variants: &[(&str, T)],
    ) -> mlua::Result<&mut Self> {
        let values = self.lua.create_table()?;
        for (name, value) in variants {
            values.set(*name, crate::arguments::value(&self.lua, value.clone())?)?;
        }
        crate::frozen::freeze(&self.lua, &values)?;
        self.lua.globals().set(type_name, values)?;
        Ok(self)
    }

    /// Run on a fresh coroutine, sharing an outer budget when imports evaluate nested chunks.
    pub(crate) fn execute(&self, function: Function, arguments: impl IntoLuaMulti) -> mlua::Result<Value> {
        if function.weak_lua() != &self.lua.weak() {
            return Err(mlua::Error::runtime("script belongs to another Lua VM"));
        }
        let outer = self.budget.active.get();
        if !outer {
            self.budget.remaining.set(self.limits.instructions_per_call);
            self.budget.exhausted.set(false);
            self.budget.active.set(self.limits.instructions_per_call != 0);
        }
        let _guard = BudgetGuard {
            budget: &self.budget,
            outer,
        };
        let thread = self.lua.create_thread(function.clone())?;
        let result = thread.resume::<Value>(arguments);
        let stopped = self.budget.exhausted.get() || thread.is_resumable();
        if result.is_err() || stopped {
            // Dropping an errored Lua 5.4 coroutine does not close its pending <close> locals. Reset it
            // while the original budget and any host loans are still active. Cleanup shares the remaining
            // quota; its failures must not replace the original runtime error or its traceback.
            let _ = thread.reset(function);
        }
        if stopped {
            return Err(mlua::Error::runtime(format!(
                "exceeded the instruction budget of {} per call",
                self.limits.instructions_per_call
            )));
        }
        result
    }

    /// An independent global namespace inheriting the current host bindings. Standard libraries and host
    /// enumeration tables are readonly, so scripts cannot change another script's bindings through them.
    pub(crate) fn environment(&self) -> mlua::Result<Table> {
        environment(&self.lua)
    }
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new(ScriptLimits::default())
    }
}
impl fmt::Debug for ScriptEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ScriptEngine").field("limits", &self.limits).finish_non_exhaustive()
    }
}

pub(crate) fn environment(lua: &Lua) -> mlua::Result<Table> {
    let environment = lua.create_table()?;
    let bindings = lua.create_table()?;
    for pair in lua.globals().pairs::<Value, Value>() {
        let (key, value) = pair?;
        if key.as_string().is_some_and(|name| name.as_bytes().as_ref() == b"__joy_require") {
            continue;
        }
        bindings.raw_set(key, value)?;
    }

    let metatable = lua.create_table()?;
    metatable.raw_set("__index", bindings)?;
    metatable.raw_set("__metatable", false)?;

    environment.set_metatable(Some(metatable))?;
    environment.raw_set("_G", environment.clone())?;
    Ok(environment)
}

pub(crate) fn ensure_owner(lua: &Lua, value: &Value) -> mlua::Result<()> {
    if value.weak_lua().is_some_and(|owner| owner != &lua.weak()) {
        return Err(mlua::Error::runtime("value belongs to another Lua VM"));
    }
    Ok(())
}

/// A trusted host callback can unwind; the next independent call must still start a fresh budget.
struct BudgetGuard<'a> {
    budget: &'a ExecutionBudget,
    outer: bool,
}
impl Drop for BudgetGuard<'_> {
    fn drop(&mut self) {
        self.budget.active.set(self.outer);
    }
}
