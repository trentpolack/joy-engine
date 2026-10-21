// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/language_information.rs`.

use super::*;

#[test]
fn every_cataloged_builtin_is_one_the_evaluator_runs() {
    let catalog = form_language_catalog();
    assert_eq!(catalog.id, "form");
    assert_eq!(catalog.symbols.len(), FORM_KEYWORDS.len() + BUILTINS.len());
    for builtin in BUILTINS {
        assert!(builtin.signature.starts_with(&format!("{}(", builtin.name)), "{} signature", builtin.name);
        // Calling with no arguments fails on arity or argument types, never as an unknown function.
        let source = format!("{}()", builtin.name);
        if let Err(error) = crate::compile(&source, &serde_json::json!({}), None) {
            assert!(!error.message.starts_with("Unknown function"), "{}: {}", builtin.name, error.message);
        }
    }
    let json = serde_json::to_value(catalog.symbol("let").unwrap()).unwrap();
    assert!(json.get("signatures").is_none());
}

#[test]
fn document_symbols_follow_block_scope_at_the_cursor() {
    let source = "param width = 2\nfn wall(ref target, height) {\n  let inner = 1\n}\nenum Kind { tower, hall }\nlet shell = geometry()\naddAttribute(shell, \"points\", \"density\", 0)\nrepeat 3 as row {\n  let column = row\n  \n}\n";
    let inside_loop = source.find("  \n}").unwrap() + 2;
    let names = |cursor: usize| -> Vec<String> {
        form_document_symbols(source, cursor).into_iter().map(|symbol| symbol.name).collect()
    };

    let visible = names(inside_loop);
    for name in ["width", "wall", "Kind", "shell", "row", "column", "density", "position"] {
        assert!(visible.contains(&name.to_owned()), "{name} should be visible in the loop");
    }
    assert!(!visible.contains(&"inner".to_owned()), "a function's locals end with its body");
    assert!(!visible.contains(&"target".to_owned()), "function parameters end with its body");

    let after = names(source.chars().count());
    assert!(!after.contains(&"column".to_owned()) && !after.contains(&"row".to_owned()));
    let wall = form_document_symbols(source, 0).into_iter().find(|symbol| symbol.name == "wall").unwrap();
    assert_eq!(wall.detail, "wall(ref target, height)");
}
