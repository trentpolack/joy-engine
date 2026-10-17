// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Editor-facing particle script entry point:
//! language information for completion and documentation, plus the compiler and its diagnostics for live validation.

pub use crate::particles::language_information::particle_language_catalog;
pub use crate::particles::script::{
    PARTICLE_CONTAINERS,
    PARTICLE_FIELDS,
    PARTICLE_FUNCTION_ARITIES,
    PARTICLE_INPUTS,
    ParticleScript,
    ParticleScriptBudget,
    ParticleScriptError,
    compile_particle_script,
};
pub use crate::scripting::{
    ScriptLanguageCatalog,
    ScriptLanguageSymbol,
    ScriptSymbolKind,
};
