// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Built-in symbol catalogs that editors, documentation, and other tooling borrow.
//!
//! Signatures are display text; consumers must not infer runtime behavior from them. Records preserve the established
//! serialized field names for JSON interchange with authoring integrations.

use serde::{
    Deserialize,
    Serialize,
};

/// Editor-neutral symbol categories, serialized as `keyword`, `function`, `variable`, or `property`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptSymbolKind {
    /// A language keyword.
    Keyword,
    /// A callable built-in.
    Function,
    /// A named value.
    Variable,
    /// A field of a built-in record.
    Property,
}

impl ScriptSymbolKind {
    /// The TypeScript spelling (`"keyword"`, `"function"`, `"variable"`, `"property"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Function => "function",
            Self::Variable => "variable",
            Self::Property => "property",
        }
    }
}

/// One built-in symbol with its documentation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLanguageSymbol {
    /// Name as written in scripts.
    pub name: String,
    /// Symbol category.
    pub kind: ScriptSymbolKind,
    /// One-line summary shown beside completions.
    pub detail: String,
    /// Longer documentation shown in hovers.
    pub documentation: String,
    /// Display signatures for callable symbols; empty when none are listed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<String>,
    /// Whether scripts may assign the symbol; `None` when the catalog does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable: Option<bool>,
}

/// A complete, editor-neutral view of one scripting language's built-in symbols.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLanguageCatalog {
    /// Language id, for example `form` or `joyfx`.
    pub id: String,
    /// Symbols in catalog order.
    pub symbols: Vec<ScriptLanguageSymbol>,
}

impl ScriptLanguageCatalog {
    /// Find a symbol by its exact name (the first one, if a catalog repeats a name).
    pub fn symbol(&self, name: &str) -> Option<&ScriptLanguageSymbol> {
        self.symbols.iter().find(|symbol| symbol.name == name)
    }
}

#[cfg(test)]
#[path = "language_information.test.rs"]
mod tests;
