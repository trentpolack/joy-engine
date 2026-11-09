// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Sandboxed Lua gameplay rules, portable math, persistent object state, and atomic hot reload.
//!
//! Hosts bind functions through [`ScriptEngine::bindings`] and describe userdata with [`lua_userdata!`]. Each script has a private environment and
//! declares hooks with explicit `this` as the first parameter. Hook arity counts the remaining host arguments.
//! Tables have reference semantics and one-based array indices; copy authored records before editing a new unit.
//!
//! ```
//! use joy_game_script::{Script, ScriptEngine, ScriptHook, ScriptState};
//! let engine = ScriptEngine::default();
//! let script = Script::compile(&engine, "charger.lua",
//!     "function tick(this, dt) this.charge = this.charge + dt * 2 end", &[ScriptHook::new("tick", 1)])?;
//! let mut state = ScriptState::new();
//! state.set(engine.lua(), "charge", 0.0).unwrap();
//! script.call(&engine, "tick", &mut state, (0.5,))?;
//! assert_eq!(state.get_as::<f64>("charge"), Some(1.0));
//! # Ok::<(), joy_game_script::ScriptError>(())
//! ```
//!
//! [`ScriptLibrary`] resolves only sources registered by the host. Data modules return export tables, which are
//! recursively frozen. Replacing source validates its transitive importers before committing any compiled program.
//! Object states remain on the same VM through reloads. [`ScriptLoan`] safely exposes host-owned state only during
//! its lending call, and [`ScriptRandom`] preserves the host's seeded draw order. [`ScriptSources`] owns a game's
//! shipped sources and accepts development edits only once the host validates the whole set; [`intern`] gives data
//! records read from modules shared `'static` text.
//!
//! Math uses `f64` and the engine's portable transcendental helpers. [`ScriptVec3`] is immutable f64 userdata;
//! Clocks, unseeded randomness, dynamic loaders, and the platform power operator are unavailable.
//! [`ScriptLimits`] bounds VM allocation and instructions. Protected calls cannot swallow an exhausted budget.
//! Memory ceilings exclude Rust allocations inside trusted host callbacks.

mod arguments;
mod convert;
pub mod definitions;
mod engine;
mod error;
mod frozen;
mod intern;
mod library;
mod loan;
mod random;
mod results;
mod script;
mod source;
mod sources;
mod standard_library;

pub use arguments::{
    IntoScriptArguments,
    IntoScriptValue,
    ScriptUserData,
};
pub use convert::{
    from_script_value,
    script_vec3,
    to_script_value,
};
pub use definitions::{
    LuaBindings,
    LuaDefinitions,
    LuaSignature,
    LuaType,
};
pub use engine::{
    ScriptEngine,
    ScriptLimits,
};
pub use error::{
    ScriptError,
    ScriptLocation,
};
pub use intern::intern;
pub use library::ScriptLibrary;
pub use loan::{
    ScriptLoan,
    ScriptResult,
};
/// The exact mlua version embedded by Joy Engine, for host bindings and Lua values.
pub use mlua;
pub use random::ScriptRandom;
pub use results::FromScriptValue;
pub use script::{
    Script,
    ScriptHook,
    ScriptState,
};
pub use sources::{
    SCRIPT_EXTENSION,
    ScriptEditError,
    ScriptSource,
    ScriptSources,
    library_from,
};
pub use standard_library::ScriptVec3;
