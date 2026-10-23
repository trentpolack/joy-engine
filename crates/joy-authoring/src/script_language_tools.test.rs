// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/script_language_tools.rs`.

use super::*;

fn kinds(syntax: &ScriptSyntax, source: &str) -> Vec<(String, TokenKind)> {
    syntax
        .tokenize(source)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Whitespace)
        .map(|token| (source[token.range].to_owned(), token.kind))
        .collect()
}

#[test]
fn form_iteration_highlighting_keeps_ranges_separate_from_decimals_and_properties() {
    let tokens = kinds(
        &ScriptSyntax::form(),
        "const count = 3 for i in 0..count { foreach item in items { item.x } } 1. .5 1e-3",
    );
    for word in ["const", "for", "foreach"] {
        assert!(tokens.contains(&(word.to_owned(), TokenKind::Keyword)), "{word}");
    }
    assert!(tokens.contains(&("0".to_owned(), TokenKind::Number)));
    assert!(tokens.contains(&("..".to_owned(), TokenKind::Operator)));
    assert!(tokens.contains(&("x".to_owned(), TokenKind::Property)));
    assert!(!tokens.contains(&("count".to_owned(), TokenKind::Property)));
    for number in ["1.", ".5", "1e-3"] {
        assert!(tokens.contains(&(number.to_owned(), TokenKind::Number)));
    }
}

#[test]
fn form_highlighting_follows_the_lab_stream_language() {
    let source = "param width = 2 meta(min=1)\n# note\nlet p = box(width, 1e-3, .5).x\n@bad \"text\" true";
    let tokens = kinds(&ScriptSyntax::form(), source);
    let find = |text: &str| tokens.iter().find(|(token, _)| token == text).map(|(_, kind)| *kind);
    assert_eq!(find("param"), Some(TokenKind::Keyword));
    assert_eq!(tokens[1], ("width".to_owned(), TokenKind::Parameter), "declarations are parameters");
    assert_eq!(tokens.iter().filter(|(text, kind)| text == "width" && *kind == TokenKind::Parameter).count(), 2);
    assert_eq!(find("meta"), Some(TokenKind::Keyword));
    assert_eq!(find("# note"), Some(TokenKind::Comment));
    assert_eq!(find("box"), Some(TokenKind::Function));
    assert_eq!(find("1e-3"), Some(TokenKind::Number));
    assert_eq!(find(".5"), Some(TokenKind::Number));
    assert_eq!(find("x"), Some(TokenKind::Property), "components after a dot");
    assert_eq!(find("@bad"), Some(TokenKind::Invalid));
    assert_eq!(find("\"text\""), Some(TokenKind::String));
    assert_eq!(find("true"), Some(TokenKind::Boolean));
    assert_eq!(find("p"), Some(TokenKind::Variable));
    let covered: usize = ScriptSyntax::form().tokenize(source).iter().map(|token| token.range.len()).sum();
    assert_eq!(covered, source.len(), "tokens cover the whole source");
}

#[test]
fn particle_and_json_highlighting() {
    let particle = ScriptSyntax::particle().with_parameters(["power"]);
    let tokens = kinds(&particle, "let a = rand(0, tau) * power; // c\ncolor.r *= 3;");
    assert_eq!(tokens[0], ("let".to_owned(), TokenKind::Keyword));
    assert!(tokens.contains(&("rand".to_owned(), TokenKind::Function)));
    assert!(tokens.contains(&("power".to_owned(), TokenKind::Parameter)));
    assert!(tokens.contains(&("// c".to_owned(), TokenKind::Comment)));
    assert!(tokens.contains(&("r".to_owned(), TokenKind::Variable)), "particle scripts color components plainly");
    let json = kinds(&ScriptSyntax::json(), "{\"rate\": -2.5e3, \"on\": true, \"x\": null}");
    assert!(json.contains(&("\"rate\"".to_owned(), TokenKind::String)));
    assert!(json.contains(&("-2.5e3".to_owned(), TokenKind::Number)));
    assert!(json.contains(&("true".to_owned(), TokenKind::Boolean)));
    assert!(json.contains(&("null".to_owned(), TokenKind::Boolean)));
}

#[test]
fn completion_filters_and_inserts_calls_with_the_cursor_inside() {
    let catalog = joy_fx::particle_language_catalog();
    let items = completion_items(&catalog);
    let matches = filter_completions(&items, "sm");
    assert_eq!(items[matches[0]].label, "smoothstep");
    assert_eq!(filter_completions(&items, "").len(), items.len(), "explicit requests list everything");
    assert!(filter_completions(&items, "zzz").is_empty());
    let mut text = "size = smo + 1".to_owned();
    let (start, word) = word_before(&text, 10);
    assert_eq!((start, word.as_str()), (7, "smo"));
    let item = &items[matches[0]];
    let cursor = apply_completion(&mut text, start, 10, item);
    assert_eq!(text, "size = smoothstep() + 1");
    assert_eq!(cursor, 18, "between the parentheses");
    let keyword = items.iter().find(|item| item.label == "let").unwrap();
    assert_eq!((keyword.insert.as_str(), keyword.cursor), ("let", 3));
    assert_eq!(word_before("x = 12", 6), (6, String::new()));
}

#[test]
fn hover_words_and_diagnostic_positions() {
    let catalog = joy_form::form_language_catalog();
    let text = "let a = sphere(1)\n  émoji😀 = box(1, 2, 3)";
    let (range, word) = word_at(text, 10).unwrap();
    assert_eq!((range, word.as_str()), (8..14, "sphere"));
    assert_eq!(symbol_heading(find_symbol(&catalog, &word).unwrap()), "sphere(radius[, segments])");
    assert!(word_at(text, 7).is_none(), "spaces and operators are not words");
    // Line 2, column 12 is `box`: columns count characters, so the emoji takes one.
    let diagnostic = ScriptDiagnostic::error(2, 12, "Unknown call");
    let range = diagnostic.character_range(text);
    let characters: Vec<char> = text.chars().collect();
    assert_eq!(characters[range].iter().collect::<String>(), "box");
    assert_eq!(line_start(text, 2), 18);
    assert_eq!(line_and_column(text, 20), (2, 3));
    assert_eq!(line_count(text), 2);
    assert_eq!(character_at(text, 9, 1), text.chars().count(), "lines past the end clamp");
}
