// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scripting/language_information.rs`.

use super::*;
use serde_json::json;

#[test]
fn catalogs_read_serialized_records() {
    let catalog: ScriptLanguageCatalog = serde_json::from_value(json!({
        "id": "joyfx",
        "symbols": [
            {"name": "sin", "kind": "function", "detail": "Sine", "documentation": "Radians.", "signatures": ["sin(x)"]},
            {"name": "age", "kind": "variable", "detail": "Age", "documentation": "Seconds.", "writable": false}
        ]
    }))
    .unwrap();
    assert_eq!(catalog.symbol("sin").map(|symbol| symbol.kind), Some(ScriptSymbolKind::Function));
    assert_eq!(catalog.symbol("age").and_then(|symbol| symbol.writable), Some(false));
    assert!(serde_json::to_value(&catalog.symbols[1]).unwrap().get("signatures").is_none());
}
