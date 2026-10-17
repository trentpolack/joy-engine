// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Content layer: editor-neutral information about Joy Engine's scripting languages (FORM and JOYFX).

pub mod language_information;

pub use language_information::{
    ScriptLanguageCatalog,
    ScriptLanguageSymbol,
    ScriptSymbolKind,
};
