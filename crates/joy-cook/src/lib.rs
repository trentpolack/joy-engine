// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine's build-time asset cook: a game's FORM scripts and instances, evaluated ahead of time into the
//! cooked-geometry bundle its release and browser builds embed (`joy_engine::cooked_forms!`), so those builds load
//! meshes instead of evaluating FORM.
//!
//! Games call [`BuildCook`] from their `build.rs`:
//!
//! ```no_run
//! joy_cook::BuildCook::new()
//!     .folder("assets/form")
//!     .skip("assets/form/garden.form") // evaluated with a per-run seed, so a default-parameter copy is never used
//!     .run();
//! ```
//!
//! Native development builds write an empty bundle, so FORM keeps evaluating live and hot reloading; release builds and
//! every browser build cook. [`FormCook`] is the cook itself, usable from tools and tests: it evaluates each
//! [`CookRequest`] (a `.form` at its defaults, or a `.forminst` with an optional variant and parameters) through the
//! same `joy_form::FormEvaluation` the runtime uses, so cooked keys match run-time lookups exactly, and keeps each
//! record in a [`CookCache`] keyed by its content hash (sources, modules, parameters, palette, and the FORM evaluator
//! version), so only edited assets evaluate again.
//!
//! What is cooked is what the build names: scripts at their authored defaults and instances at their own overrides and
//! each variant, plus explicit requests. Evaluations whose inputs a game picks at run time (seeded layouts, paint, or
//! palettes built in code) find no cooked copy and evaluate live in every build.

mod build_script;
mod cook_cache;
mod form_cook;

pub use build_script::{
    BuildCook,
    CACHE_DIRECTORY_VARIABLE,
    COOK_MODE_VARIABLE,
    CookMode,
};
pub use cook_cache::CookCache;
pub use form_cook::{
    CookFailure,
    CookReport,
    CookRequest,
    CookedEntry,
    FormCook,
    SkippedEntry,
};
