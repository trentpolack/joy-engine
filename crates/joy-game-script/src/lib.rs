// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Game scripts: gameplay code written in [Rhai](https://rhai.rs) that hosts compile at run time, so native
//! development builds can hot reload behavior without rebuilding the game. Renderer-free and headless; the same scripts
//! run natively and in WebAssembly.
//!
//! Data flow: a host builds one [`ScriptEngine`] (the sandboxed language plus the engine's standard library), registers
//! its own types and functions on it, and compiles authored source into a [`Script`] that must declare the
//! [`ScriptHook`]s the host calls. Each scripted object owns a [`ScriptState`], the object map its hooks see as
//! `this`, so state survives between calls and across hot reloads. Host state that scripts may query during a call
//! (a world, a random stream) is lent through a [`ScriptLoan`] and returned when the call ends.
//!
//! ```
//! use joy_game_script::{
//!     Script, ScriptEngine, ScriptHook, ScriptLimits, ScriptState,
//! };
//!
//! const HOOKS: [ScriptHook; 1] = [ScriptHook::new("tick", 1)];
//! let engine = ScriptEngine::new(ScriptLimits::default());
//! let source = "const RATE = 2.0; fn tick(elapsed_seconds) { this.charge += elapsed_seconds * global::RATE; }";
//! let script = Script::compile(&engine, "charger.rhai", source, &HOOKS)?;
//!
//! let mut state = ScriptState::new();
//! state.set("charge", 0.0);
//! script.call(&engine, "tick", &mut state, (0.5,))?;
//! assert_eq!(state.get_as::<f64>("charge"), Some(1.0));
//! # Ok::<(), joy_game_script::ScriptError>(())
//! ```
//!
//! # Determinism
//!
//! Scripts compute in `f64`. Arithmetic, `sqrt`, and vector lengths are exact on every target, and the standard
//! library routes transcendental functions through `joy_core::math::deterministic`, so a script that draws from a
//! seeded stream ([`ScriptRandom`]) replays identically natively and in WebAssembly. Rhai's own math package, its
//! floating-point `**` operator, and its clock functions are left out because they are not portable.
//!
//! # Sandbox and Limits
//!
//! Scripts cannot read files, load modules, or read the clock. Every hook call runs under [`ScriptLimits`]: an
//! operation budget that stops runaway loops, call and expression depth limits, and collection size limits. A limit
//! ends only that call, with a [`ScriptError`].
//!
//! # Binding Host Types
//!
//! Hosts register types and functions on [`ScriptEngine::rhai_mut`] with Rhai's own API, re-exported as [`rhai`] so
//! the engine pins one version; [`ScriptEngine::register_enum`] covers enumerations. Pass small plain views by value
//! and lend larger owned state through a [`ScriptLoan`]; script parameters are copies, so only `this` is mutable.

mod engine;
mod error;
mod loan;
mod random;
mod script;
mod standard_library;

pub use engine::{
    ScriptEngine,
    ScriptLimits,
};
pub use error::{
    ScriptError,
    ScriptLocation,
};
pub use loan::{
    ScriptLoan,
    ScriptResult,
};
pub use random::ScriptRandom;
/// The Rhai crate the engine embeds, for registering host types and functions and reading values scripts return.
pub use rhai;
pub use script::{
    Script,
    ScriptHook,
    ScriptState,
};
