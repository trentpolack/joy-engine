// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Parser grammar, character-column diagnostics, metadata validation, and source budgets.

use joy_form::{
    CompiledScript,
    compile,
};
use serde_json::json;

fn assert_error(source: &str, message: &str, line: usize, column: usize) {
    let error = match CompiledScript::parse(source) {
        Ok(_) => panic!("Expected parse failure for {source:?}"),
        Err(error) => error,
    };
    assert_eq!(error.message, message, "source: {source:?}");
    assert_eq!((error.line, error.column), (line, column));
}

#[test]
fn loops_distinguish_ranges_collections_and_retired_repeat_syntax() {
    for (source, guidance) in [
        ("repeat 3 as i {}", "for"),
        ("for item in [1,2] {}", "foreach"),
        ("foreach item in 0..3 {}", "for"),
        ("for i in 0..=3 {}", "exclusive"),
    ] {
        let error = CompiledScript::parse(source).unwrap_err();
        assert!(error.message.contains(guidance), "{source}: {error}");
    }
    assert!(CompiledScript::parse("for i in 0..3 {} foreach item in [{x:1}] { const x=item.x }").is_ok());
}

#[test]
fn parses_complete_statement_and_data_syntax() {
    let source = r#"
        # Headers and semicolons can separate statements.
        project meta(name="Parser study", title="A \"title\"", info="line\nline");;
        param width = 2 meta(min=0, max=10, step=.25)
        param height = 3, 0, 10, 1
        let data = {"display name": "hello", nested: [true, false, null, {}, []]}
        use "parts/kit.form" as kit
        enum Kind { tower, hall, }
        fn shell(ref target, size) {
            append(target, box(size, size, size))
            return
        }
        let g = geometry()
        let count = 0
        for i in 0..width {
            if i >= 1 AND NOT (i == 3) { count += 1 } else if i == 0 OR false { count -= 1 } else { count *= 2 }
        }
        match Kind.hall { case Kind.tower, Kind.hall { count /= 2 } else { } }
        wrangle points in g as p {
            p.position = vector(p.position.x + 1, 0, 0)
            p.position += vector(0, data["nested"][0], 0)
        }
        shell(ref g, kit.unit)
        // Names are interpreted as functions only during evaluation.
        arbitrary(1e-2, 5., +3, (width + height))
    "#;
    let error = CompiledScript::parse(source).unwrap_err();
    assert!(error.message.starts_with("Modules need a host that loads files"), "parsed everything else: {error}");
    assert!(CompiledScript::parse(&source.replace("use \"parts/kit.form\" as kit", "")).is_ok());
}

#[test]
fn grammar_words_and_misplaced_declarations_are_rejected_where_they_appear() {
    assert_error("let AND = 1", "'AND' is a reserved word and cannot be used as a name.", 1, 5);
    assert_error(
        "let x = 1
else { }",
        "'else' must follow an if block or a match case.",
        2,
        1,
    );
    assert_error("for i in 0..2 { fn f() { } }", "Declare functions at the top level of a script.", 1, 17);
    assert_error("return 1", "'return' is only allowed inside a function.", 1, 1);
    assert_error("if true { enum A { x } }", "Declare enums at the top level of a script.", 1, 11);
    assert_error("enum A { }", "An enum requires 1 to 32 members.", 1, 8);
    assert_error("enum A { x, x }", "Duplicate enum member 'x'.", 1, 13);
    assert_error("fn f(a, a) { }", "Duplicate function parameter 'a'.", 1, 9);
    assert_error("match 1 { else { } case 1 { } }", "A match's else block must come last.", 1, 20);
    assert_error(
        "let x = 1
x + 1",
        "Expected a statement; a value on its own must be a function call.",
        2,
        1,
    );
}

#[test]
fn exponentiation_binds_more_tightly_than_unary_and_associates_right() {
    let geometry =
        compile("let g = geometry() addPoint(g, vector(-2^2, 2^3^2, 7%4+8/2*3)) output(\"P\", g)", &json!({}), None)
            .unwrap();
    assert_eq!(geometry.positions, vec![-4.0, 512.0, 15.0]);
}

#[test]
fn reports_character_columns_and_comment_line_boundaries() {
    assert_error(
        "let s = \"😀\"; let x = @",
        "@attribute syntax is no longer supported. Use 'wrangle points in geometry as point' and 'point.name'; declare fields first with addAttribute().",
        1,
        22,
    );
    assert_error("#😀\r\nlet s = \"😀\"; let x = ;", "Expected an expression, found ';'.", 2, 22);
    // A leading byte-order mark takes no column; other Unicode white space does.
    assert_error("\u{feff}\u{00a0}let x = )", "Expected an expression, found ')'.", 1, 10);
    assert_error("\u{2603}", "Unexpected character '\u{2603}'.", 1, 1);
}

#[test]
fn rejects_invalid_metadata_and_data_keys_at_the_original_tokens() {
    assert_error("project meta(title=\"T\")", "Project metadata requires a name.", 1, 1);
    assert_error("project meta(name=\"\")", "Project name requires nonempty, single-line text.", 1, 19);
    assert_error("project meta(name=1)", "Project metadata requires double-quoted text.", 1, 19);
    assert_error("project meta(name=\"X\", name=\"Y\")", "Duplicate project metadata 'name'.", 1, 24);
    assert_error("project meta(other=\"X\")", "Unknown project metadata 'other'.", 1, 14);
    assert_error("param x = 1 meta(min=0,min=2)", "Duplicate metadata 'min'.", 1, 27);
    assert_error("param x = 1 meta(color=2)", "Unknown parameter metadata 'color'.", 1, 23);
    assert_error("param x = 1 meta(label=2)", "Parameter metadata text requires double quotes.", 1, 24);
    assert_error("param x = 1 meta(options=[\"Only\"])", "Parameter options require 2 to 32 labels.", 1, 26);
    assert_error("let x = {a:1,a:2}", "Duplicate record field 'a'.", 1, 14);
    assert_error(
        "let x = {constructor:1}",
        "Invalid record field name (1–64 characters; prototype keys are reserved).",
        1,
        10,
    );
    assert_error(
        "let x = {\"\":1}",
        "Invalid record field name (1–64 characters; prototype keys are reserved).",
        1,
        10,
    );
}

#[test]
fn rejects_trailing_commas_and_unterminated_blocks() {
    assert_error("f(1,)", "Expected an expression, found ')'.", 1, 5);
    assert_error("let x = [1,]", "Expected an expression, found ']'.", 1, 12);
    assert_error("let x = {a:1,}", "Expected a variable name.", 1, 14);
    assert_error("for i in 0..2 {", "Expected '}', found '<end>'.", 1, 16);
    assert_error("}", "Unexpected '}'.", 1, 1);
    assert_error("let x = \"bad\\q\"", "Invalid quoted text.", 1, 9);
    assert_error("let x = \"line\nline\"", "Unexpected character '\"'.", 1, 9);
}

#[test]
fn reports_quoted_record_key_errors_at_the_offending_character() {
    assert_error("let x = {\"a\\q\":1}", "Bad escaped character in quoted key at character 4.", 1, 10);
    assert_error("let x = {\"😀\\u12z4\":1}", "Bad Unicode escape in quoted key at character 7.", 1, 10);
    assert_error(
        "let x = {\"a\tb\":1}",
        "Bad control character in string literal in quoted key at character 3.",
        1,
        10,
    );
}

#[test]
fn bounds_source_tokens_and_recursive_nesting() {
    assert!(CompiledScript::parse(&";".repeat(40_000)).is_ok());
    assert_error(&";".repeat(40_001), "Token limit exceeded.", 1, 40_001);
    assert_error(&format!("#{}", "😀".repeat(200_000)), "Script size limit is 200,000 characters.", 1, 1);
    assert!(CompiledScript::parse(&format!("let a = {}1", "+".repeat(62))).is_ok());
    assert_error(&format!("let a = {}1", "+".repeat(63)), "Expression nesting limit exceeded.", 1, 72);
    assert!(CompiledScript::parse(&format!("{}{}", "if true {".repeat(47), "}".repeat(47))).is_ok());
    assert_error(&format!("{}{}", "if true {".repeat(48), "}".repeat(48)), "Nesting limit exceeded.", 1, 433);
}
