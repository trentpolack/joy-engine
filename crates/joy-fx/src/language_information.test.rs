// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/language_information.rs`.

use super::*;

#[test]
fn catalog_describes_every_function_field_and_input() {
    let catalog = particle_language_catalog();
    let find = |name: &str| catalog.symbols.iter().find(|symbol| symbol.name == name);
    for (name, _) in PARTICLE_FUNCTION_ARITIES {
        assert_eq!(find(name).map(|symbol| symbol.kind), Some(ScriptSymbolKind::Function), "{name}");
    }
    for name in PARTICLE_FIELDS {
        assert_eq!(find(name).and_then(|symbol| symbol.writable), Some(true), "{name}");
    }
    for name in PARTICLE_INPUTS {
        assert_eq!(find(name).and_then(|symbol| symbol.writable), Some(false), "{name}");
    }
    assert_eq!(find("rand").unwrap().detail, "rand() | rand(value1, value2)");
}
