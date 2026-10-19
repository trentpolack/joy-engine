// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/language_information.rs`.

use super::*;

#[test]
fn catalog_matches_the_typescript_records() {
    let catalog = form_language_catalog();
    assert_eq!(catalog.id, "form");
    assert_eq!(catalog.symbols.len(), 10 + FORM_COMMANDS.len(), "userData is listed once, as a command");
    assert_eq!(catalog.symbol("param").map(|symbol| symbol.kind), Some(ScriptSymbolKind::Keyword));
    assert_eq!(catalog.symbol("userData").map(|symbol| symbol.kind), Some(ScriptSymbolKind::Function));
    let sphere = catalog.symbol("sphere").unwrap();
    assert_eq!(sphere.detail, "sphere(radius[, segments])");
    assert_eq!(catalog.symbol("revolve").unwrap().signatures, vec!["revolve(…)".to_owned()]);
    let json = serde_json::to_value(catalog.symbol("let").unwrap()).unwrap();
    assert_eq!(json["documentation"], "The let keyword is part of the FORM language grammar.");
    assert!(json.get("signatures").is_none());
}
