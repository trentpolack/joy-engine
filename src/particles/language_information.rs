// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Editor-neutral information about the particle script language's built-in symbols, for completion, hover text, and
//! documentation. Generated from the same tables the compiler uses, so it cannot drift from the runtime. Records are the
//! shared `scripting` catalog types, so editors treat FORM and JOYFX alike.

use crate::particles::script::{PARTICLE_FIELDS, PARTICLE_FUNCTION_ARITIES, PARTICLE_INPUTS};
use crate::scripting::{ScriptLanguageCatalog, ScriptLanguageSymbol, ScriptSymbolKind};

/// Language keywords.
const KEYWORDS: [&str; 3] = ["let", "if", "else"];

/// Build the particle language catalog; the TypeScript `PARTICLE_LANGUAGE_CATALOG`.
pub fn particle_language_catalog() -> ScriptLanguageCatalog {
    let keywords = KEYWORDS.iter().map(|name| ScriptLanguageSymbol {
        name: (*name).to_owned(),
        kind: ScriptSymbolKind::Keyword,
        detail: "Particle script keyword".to_owned(),
        documentation: format!("The {name} keyword controls bounded particle-script execution."),
        signatures: Vec::new(),
        writable: None,
    });
    let functions = PARTICLE_FUNCTION_ARITIES.iter().map(|(name, arities)| {
        let signatures: Vec<String> = arities
            .iter()
            .map(|arity| {
                let arguments: Vec<String> = (1..=*arity).map(|index| format!("value{index}")).collect();
                format!("{name}({})", arguments.join(", "))
            })
            .collect();
        let return_kind = match *name {
            "vector" => "vector",
            "rgba" => "color",
            _ => "scalar",
        };
        ScriptLanguageSymbol {
            name: (*name).to_owned(),
            kind: ScriptSymbolKind::Function,
            detail: signatures.join(" | "),
            documentation: format!("The {name} built-in returns a bounded {return_kind} value."),
            signatures,
            writable: None,
        }
    });
    let fields = PARTICLE_FIELDS.iter().map(|name| ScriptLanguageSymbol {
        name: (*name).to_owned(),
        kind: ScriptSymbolKind::Property,
        detail: "Writable particle scalar".to_owned(),
        documentation: format!("Reads or updates the current particle's {name} value."),
        signatures: Vec::new(),
        writable: Some(true),
    });
    let inputs = PARTICLE_INPUTS.iter().map(|name| ScriptLanguageSymbol {
        name: (*name).to_owned(),
        kind: ScriptSymbolKind::Variable,
        detail: "Read-only particle input".to_owned(),
        documentation: format!("Reads the {name} value supplied by the particle simulation."),
        signatures: Vec::new(),
        writable: Some(false),
    });
    ScriptLanguageCatalog {
        id: "particle".to_owned(),
        symbols: keywords.chain(functions).chain(fields).chain(inputs).collect(),
    }
}

#[cfg(test)]
mod tests {
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
}
