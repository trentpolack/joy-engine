// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Script language catalogs (the applicable part of TypeScript `script-language-tools.test.mjs`): hover adapters
//! resolve exact symbols. Completion presentation moved to the shared authoring kit's script editor.

use joy_engine::scripting::{ScriptLanguageCatalog, ScriptLanguageSymbol, ScriptSymbolKind};

#[test]
fn catalogs_resolve_exact_symbols_for_hover_documentation() {
    let symbol = |name: &str, kind, detail: &str| ScriptLanguageSymbol {
        name: name.to_owned(),
        kind,
        detail: detail.to_owned(),
        documentation: format!("{name} documentation."),
        signatures: Vec::new(),
        writable: None,
    };
    let catalog = ScriptLanguageCatalog {
        id: "test".to_owned(),
        symbols: vec![
            symbol("let", ScriptSymbolKind::Keyword, "Keyword"),
            symbol("noise", ScriptSymbolKind::Function, "noise(value)"),
        ],
    };
    assert_eq!(catalog.symbol("noise").map(|symbol| symbol.detail.as_str()), Some("noise(value)"));
    assert!(catalog.symbol("no").is_none());
    assert_eq!(catalog.symbols.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(), ["let", "noise"]);
}
