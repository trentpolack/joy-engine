// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Editor-neutral information about Joy Engine's scripting languages (FORM and JOYFX). It lives in the foundation so
//! FORM, particles, and the script editor share one vocabulary without depending on each other.

pub mod language_information;

pub use language_information::{
    ScriptLanguageCatalog,
    ScriptLanguageSymbol,
    ScriptSymbolKind,
};
