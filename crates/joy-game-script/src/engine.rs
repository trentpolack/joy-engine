// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The sandboxed script language: one Rhai engine configured for deterministic, file-free gameplay code, plus the
//! limits every hook call runs under. Hosts own one [`ScriptEngine`] per set of bindings and register their types on it
//! before compiling scripts.

use std::fmt;
use std::rc::Rc;

use rhai::Engine;
use rhai::Module;
use rhai::module_resolvers::DummyModuleResolver;
use rhai::packages::BasicArrayPackage;
use rhai::packages::BasicMapPackage;
use rhai::packages::CorePackage;
use rhai::packages::Package;

use crate::standard_library;

/// Operations one hook call may run before it stops (a `while true {}` stops in well under a millisecond).
pub const DEFAULT_OPERATIONS_PER_CALL: u64 = 200_000;

/// Nested script function calls per hook call; matches FORM's call depth.
pub const DEFAULT_CALL_DEPTH: usize = 32;

/// Expression nesting at the top level and inside functions.
pub const DEFAULT_EXPRESSION_DEPTH: usize = 64;

/// Characters in one string, elements in one array, and entries in one object map.
pub const DEFAULT_COLLECTION_SIZE: usize = 10_000;

/// Execution limits applied to every hook call. A limit ends only the call that exceeded it, with a
/// [`crate::ScriptError::Runtime`]; the script and its state remain usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptLimits {
    /// Operations (roughly expressions and statements) one call may run; stops runaway loops.
    pub operations_per_call: u64,
    /// Nested script function calls, including recursion.
    pub call_depth: usize,
    /// Expression nesting depth.
    pub expression_depth: usize,
    /// Largest string, array, or object map a script may build.
    pub collection_size: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        Self {
            operations_per_call: DEFAULT_OPERATIONS_PER_CALL,
            call_depth: DEFAULT_CALL_DEPTH,
            expression_depth: DEFAULT_EXPRESSION_DEPTH,
            collection_size: DEFAULT_COLLECTION_SIZE,
        }
    }
}

/// The game script language: Rhai's core language with the engine's portable standard library and no access to files,
/// modules, or the clock. Registered host types and functions are shared by every [`crate::Script`] compiled for this
/// engine; register them before calling hooks that use them.
pub struct ScriptEngine {
    engine: Engine,
    limits: ScriptLimits,
}

impl ScriptEngine {
    /// A sandboxed engine with the standard library (`vec3`, deterministic math, and [`crate::ScriptRandom`]) and
    /// `limits` applied to every call. `print` and `debug` go to the `log` facade at info and debug levels.
    pub fn new(limits: ScriptLimits) -> Self {
        let mut engine = Engine::new_raw();

        // Language core: operators, strings, iteration, function pointers, arrays, and object maps. Rhai's math and
        // time packages are left out on purpose: their transcendental functions call the platform's `f64` methods,
        // which differ between native and WebAssembly builds, and the clock would make replays diverge.
        engine.register_global_module(CorePackage::new().as_shared_module());
        engine.register_global_module(BasicArrayPackage::new().as_shared_module());
        engine.register_global_module(BasicMapPackage::new().as_shared_module());
        standard_library::register(&mut engine);

        // Registered operators must win over Rhai's built-ins, whose float comparisons are approximate (see
        // `standard_library::register_exact_comparisons`). Arithmetic still uses the built-ins, which are exact.
        engine.set_fast_operators(false);

        // Sandbox: no `import` of other files, and floating-point `**` (the platform `powf`) is replaced by `powf()`.
        engine.set_module_resolver(DummyModuleResolver::new());
        engine.disable_symbol("**");

        // Reading a state field that was never set is an error rather than a silent `()`, so typos surface at once.
        engine.set_fail_on_invalid_map_property(true);

        engine.on_print(|text| log::info!("[script] {text}"));
        engine.on_debug(|text, source, position| {
            log::debug!("[script] {} {position}: {text}", source.unwrap_or("<script>"));
        });

        let mut script_engine = Self {
            engine,
            limits,
        };
        script_engine.set_limits(limits);
        script_engine
    }

    /// The limits every call runs under.
    pub fn limits(&self) -> ScriptLimits {
        self.limits
    }

    /// Replace the limits for later calls.
    pub fn set_limits(&mut self, limits: ScriptLimits) {
        self.limits = limits;
        self.engine.set_max_operations(limits.operations_per_call);
        self.engine.set_max_call_levels(limits.call_depth);
        self.engine.set_max_expr_depths(limits.expression_depth, limits.expression_depth);
        self.engine.set_max_string_size(limits.collection_size);
        self.engine.set_max_array_size(limits.collection_size);
        self.engine.set_max_map_size(limits.collection_size);
    }

    /// The underlying Rhai engine, for compiling and calling (crate use) and inspection.
    pub fn rhai(&self) -> &Engine {
        &self.engine
    }

    /// The underlying Rhai engine, for registering host types (`register_type_with_name`, `register_get`) and functions
    /// (`register_fn`). Registering is the intended use; changing the sandbox or limits here bypasses
    /// [`ScriptEngine::set_limits`].
    pub fn rhai_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Register a host enumeration as script type `type_name`: each variant becomes a constant read as
    /// `TypeName::Variant`, values compare with `==` and `!=`, and `to_string` returns the variant's script name.
    /// Variant names are the script spellings, independent of any serialized spelling the host uses.
    pub fn register_enum<T>(&mut self, type_name: &str, variants: &[(&str, T)]) -> &mut Self
    where
        T: Clone + PartialEq + fmt::Debug + 'static,
    {
        let names: Rc<Vec<(String, T)>> =
            Rc::new(variants.iter().map(|(name, value)| ((*name).to_string(), value.clone())).collect());

        let mut module = Module::new();
        for (name, value) in names.iter() {
            module.set_var(name.as_str(), value.clone());
        }

        let display_names = Rc::clone(&names);
        let debug_names = Rc::clone(&names);
        self.engine
            .register_type_with_name::<T>(type_name)
            .register_fn("==", |left: &mut T, right: T| *left == right)
            .register_fn("!=", |left: &mut T, right: T| *left != right)
            .register_fn("to_string", move |value: &mut T| variant_name(&display_names, value))
            .register_fn("to_debug", move |value: &mut T| variant_name(&debug_names, value));
        self.engine.register_static_module(type_name, module.into());
        self
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

/// The registered script name of an enumeration value, or its Rust debug spelling for an unregistered variant.
fn variant_name<T: PartialEq + fmt::Debug>(names: &[(String, T)], value: &T) -> String {
    match names.iter().find(|(_, variant)| variant == value) {
        Some((name, _)) => name.clone(),
        None => format!("{value:?}"),
    }
}
